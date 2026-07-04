use std::path::Path;

use ffmpeg::format::{self, Pixel};
use ffmpeg::software::scaling::{Context as Scaler, Flags};
use ffmpeg::{Dictionary, Packet, Rational, Rescale, codec, encoder, frame, media, picture};
use ffmpeg_next as ffmpeg;

use crate::export::{ExportData, FrameProcessor, VideoExportInfo};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

struct AudioCopy {
    in_index: usize,
    in_tb: Rational,
    ost_index: usize,
    ost_tb: Rational,
}

struct Output {
    octx: format::context::Output,
    encoder: encoder::Video,
    enc_tb: Rational,
    video_ost_tb: Rational,
    audio: Option<AudioCopy>,
}

const VIDEO_OST_INDEX: usize = 0;

fn open_output(
    path: &Path,
    enc_w: u32,
    enc_h: u32,
    in_tb: Rational,
    frame_rate: Rational,
    audio: Option<(usize, Rational, codec::Parameters)>,
) -> Result<Output, String> {
    let known_container = matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("mp4" | "mkv" | "mov")
    );
    let mut octx = if known_container {
        format::output(path).map_err(err)?
    } else {
        format::output_as(path, "mp4").map_err(err)?
    };
    let global_header = octx.format().flags().contains(format::Flags::GLOBAL_HEADER);

    let codec = encoder::find(codec::Id::H264)
        .ok_or_else(|| "This FFmpeg build has no H.264 encoder.".to_string())?;
    octx.add_stream(codec).map_err(err)?;

    let mut enc_ctx = codec::context::Context::new_with_codec(codec);
    enc_ctx.set_threading(codec::threading::Config::kind(
        codec::threading::Type::Frame,
    ));
    let mut enc = enc_ctx.encoder().video().map_err(err)?;
    enc.set_width(enc_w);
    enc.set_height(enc_h);
    enc.set_format(Pixel::YUV420P);
    enc.set_time_base(in_tb);
    if frame_rate.numerator() > 0 && frame_rate.denominator() > 0 {
        enc.set_frame_rate(Some(frame_rate));
    }
    if global_header {
        enc.set_flags(codec::Flags::GLOBAL_HEADER);
    }

    let mut opts = Dictionary::new();
    opts.set("preset", "medium");
    opts.set("crf", "18");
    let opened = enc.open_with(opts).map_err(err)?;
    octx.stream_mut(VIDEO_OST_INDEX)
        .ok_or_else(|| "Missing output stream.".to_string())?
        .set_parameters(&opened);
    let enc_tb = opened.time_base();

    let audio_ost = match &audio {
        Some((_, _, params)) => {
            let mut ost = octx
                .add_stream(encoder::find(codec::Id::None))
                .map_err(err)?;
            ost.set_parameters(params.clone());
            unsafe {
                (*ost.parameters().as_mut_ptr()).codec_tag = 0;
            }
            Some(ost.index())
        }
        None => None,
    };

    octx.write_header().map_err(err)?;

    let video_ost_tb = octx
        .stream(VIDEO_OST_INDEX)
        .map(|s| s.time_base())
        .unwrap_or(in_tb);
    let audio = audio.and_then(|(in_index, in_tb, _)| {
        let ost_index = audio_ost?;
        let ost_tb = octx.stream(ost_index)?.time_base();
        Some(AudioCopy {
            in_index,
            in_tb,
            ost_index,
            ost_tb,
        })
    });

    Ok(Output {
        octx,
        encoder: opened,
        enc_tb,
        video_ost_tb,
        audio,
    })
}

fn copy_from_frame(frame: &frame::Video, buf: &mut [u8], w: u32, h: u32) {
    let stride = frame.stride(0);
    let row = w as usize * 4;
    let src = frame.data(0);
    if stride == row {
        buf.copy_from_slice(&src[..row * h as usize]);
    } else {
        for y in 0..h as usize {
            buf[y * row..(y + 1) * row].copy_from_slice(&src[y * stride..y * stride + row]);
        }
    }
}

fn copy_to_frame(buf: &[u8], frame: &mut frame::Video, w: u32, h: u32) {
    let stride = frame.stride(0);
    let row = w as usize * 4;
    let dst = frame.data_mut(0);
    if stride == row {
        dst[..row * h as usize].copy_from_slice(buf);
    } else {
        for y in 0..h as usize {
            dst[y * stride..y * stride + row].copy_from_slice(&buf[y * row..(y + 1) * row]);
        }
    }
}

fn write_encoded(out: &mut Output) -> Result<(), String> {
    let mut packet = Packet::empty();
    while out.encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(VIDEO_OST_INDEX);
        packet.rescale_ts(out.enc_tb, out.video_ost_tb);
        packet.write_interleaved(&mut out.octx).map_err(err)?;
    }
    Ok(())
}

pub fn encode_video(
    data: &ExportData,
    info: &VideoExportInfo,
    path: &Path,
    progress: &impl Fn(f32),
) -> Result<(), String> {
    ffmpeg::init().map_err(err)?;

    let mut ictx = format::input(&info.path).map_err(err)?;
    let (video_index, in_tb, frame_rate) = {
        let ist = ictx
            .streams()
            .best(media::Type::Video)
            .ok_or_else(|| "No video stream.".to_string())?;
        (ist.index(), ist.time_base(), ist.avg_frame_rate())
    };
    let audio_in = ictx
        .streams()
        .best(media::Type::Audio)
        .map(|s| (s.index(), s.time_base(), s.parameters()));

    let mut decoder_ctx = codec::context::Context::from_parameters(
        ictx.stream(video_index)
            .ok_or_else(|| "No video stream.".to_string())?
            .parameters(),
    )
    .map_err(err)?;
    decoder_ctx.set_threading(codec::threading::Config::kind(
        codec::threading::Type::Frame,
    ));
    let mut decoder = decoder_ctx.decoder().video().map_err(err)?;

    if decoder.width() != data.width || decoder.height() != data.height {
        return Err("Video dimensions changed on disk since it was opened.".to_string());
    }

    let processor = FrameProcessor::new(data);
    let (out_w, out_h) = processor.out_size();
    let enc_w = out_w.max(2) & !1;
    let enc_h = out_h.max(2) & !1;

    let mut out = match open_output(path, enc_w, enc_h, in_tb, frame_rate, audio_in.clone()) {
        Ok(o) => o,
        Err(e) if audio_in.is_some() => {
            open_output(path, enc_w, enc_h, in_tb, frame_rate, None).map_err(|_| e)?
        }
        Err(e) => return Err(e),
    };

    let mut to_rgba = Scaler::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        Pixel::RGBA,
        decoder.width(),
        decoder.height(),
        Flags::BILINEAR,
    )
    .map_err(err)?;
    let mut to_yuv = Scaler::get(
        Pixel::RGBA,
        out_w,
        out_h,
        Pixel::YUV420P,
        enc_w,
        enc_h,
        Flags::BILINEAR,
    )
    .map_err(err)?;

    let mut decoded = frame::Video::empty();
    let mut rgba = frame::Video::empty();
    let mut yuv = frame::Video::empty();
    let mut out_frame = frame::Video::new(Pixel::RGBA, out_w, out_h);
    let mut src_buf = vec![0u8; data.width as usize * data.height as usize * 4];
    let mut out_buf = vec![0u8; out_w as usize * out_h as usize * 4];

    let mut frames_done: u64 = 0;
    let mut last_pts: Option<i64> = None;
    let duration_ts = {
        let secs = info.duration.as_secs_f64();
        let tb = if in_tb.denominator() != 0 {
            in_tb.numerator() as f64 / in_tb.denominator() as f64
        } else {
            0.0
        };
        if tb > 0.0 { secs / tb } else { 0.0 }
    };

    macro_rules! drain_decoder {
        () => {
            while decoder.receive_frame(&mut decoded).is_ok() {
                let mut pts = decoded
                    .pts()
                    .or_else(|| decoded.timestamp())
                    .unwrap_or_else(|| last_pts.map_or(0, |p| p.saturating_add(1)));
                if let Some(last) = last_pts
                    && pts <= last
                {
                    pts = last.saturating_add(1);
                }
                last_pts = Some(pts);

                to_rgba.run(&decoded, &mut rgba).map_err(err)?;
                copy_from_frame(&rgba, &mut src_buf, data.width, data.height);
                processor.process_into(&src_buf, &mut out_buf)?;
                copy_to_frame(&out_buf, &mut out_frame, out_w, out_h);
                let ret = unsafe {
                    if yuv.is_empty() {
                        0
                    } else {
                        ffmpeg::ffi::av_frame_make_writable(yuv.as_mut_ptr())
                    }
                };
                if ret < 0 {
                    return Err(err(ffmpeg::Error::from(ret)));
                }
                to_yuv.run(&out_frame, &mut yuv).map_err(err)?;
                yuv.set_pts(Some(pts.rescale(in_tb, out.enc_tb)));
                yuv.set_kind(picture::Type::None);
                out.encoder.send_frame(&yuv).map_err(err)?;
                write_encoded(&mut out)?;

                frames_done += 1;
                if info.frame_count > 0 {
                    progress((frames_done as f32 / info.frame_count as f32).min(1.0));
                } else if duration_ts > 0.0 {
                    progress((pts as f64 / duration_ts).clamp(0.0, 1.0) as f32);
                }
            }
        };
    }

    for (stream, mut packet) in ictx.packets() {
        let index = stream.index();
        if index == video_index {
            decoder.send_packet(&packet).map_err(err)?;
            drain_decoder!();
        } else if let Some(audio) = &out.audio
            && index == audio.in_index
        {
            packet.rescale_ts(audio.in_tb, audio.ost_tb);
            packet.set_stream(audio.ost_index);
            packet.set_position(-1);
            packet.write_interleaved(&mut out.octx).map_err(err)?;
        }
    }

    let _ = decoder.send_eof();
    drain_decoder!();

    if frames_done == 0 {
        return Err("No decodable video frames.".to_string());
    }

    out.encoder.send_eof().map_err(err)?;
    write_encoded(&mut out)?;
    out.octx.write_trailer().map_err(err)?;
    progress(1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use crate::export::{ExportData, ExportSource, VideoExportInfo, do_export};
    use crate::modifiers::kinds::Exposure;
    use crate::modifiers::{Modifier, ModifierKind};
    use crate::wgpu::media::video::probe_video;

    fn ffmpeg_exe() -> Option<std::path::PathBuf> {
        let p =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/ffmpeg/bin/ffmpeg.exe");
        p.exists().then_some(p)
    }

    #[test]
    #[ignore]
    fn perf_bench() {
        let ffmpeg_exe = ffmpeg_exe().expect("vendored ffmpeg");
        let dir = std::env::temp_dir().join("bloom_video_export_bench");
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.mp4");
        if !input.exists() {
            let status = Command::new(&ffmpeg_exe)
                .args([
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=duration=5:size=1920x1080:rate=30",
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                ])
                .arg(&input)
                .status()
                .unwrap();
            assert!(status.success());
        }
        let info = probe_video(&input).expect("probe");
        let data = ExportData {
            source: ExportSource::Video(VideoExportInfo {
                path: input.clone(),
                frame_count: info.frame_count,
                duration: info.duration,
            }),
            width: info.width,
            height: info.height,
            modifiers: vec![],
            crop: None,
            rotation: 0,
        };
        let t = std::time::Instant::now();
        do_export(data, &dir.join("out.mp4"), |_| {}).expect("export");
        eprintln!("1080p30 5s export took {:?}", t.elapsed());
    }

    #[test]
    fn video_export_crops_rotates_and_keeps_audio() {
        let Some(ffmpeg_exe) = ffmpeg_exe() else {
            eprintln!("vendored ffmpeg.exe not found, skipping");
            return;
        };
        let dir = std::env::temp_dir().join("bloom_video_export_test");
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.mp4");
        let output = dir.join("output.mp4");

        let status = Command::new(&ffmpeg_exe)
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:size=64x48:duration=1:rate=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .arg(&input)
            .status()
            .expect("run ffmpeg");
        assert!(status.success(), "sample generation failed");

        let info = probe_video(&input).expect("probe input");
        let data = ExportData {
            source: ExportSource::Video(VideoExportInfo {
                path: input.clone(),
                frame_count: info.frame_count,
                duration: info.duration,
            }),
            width: info.width,
            height: info.height,
            modifiers: vec![],
            crop: Some([0.25, 0.25, 0.75, 0.75]),
            rotation: 1,
        };
        do_export(data, &output, |_| {}).expect("export");

        let out_info = probe_video(&output).expect("probe output");
        assert_eq!((out_info.width, out_info.height), (24, 32));
        assert!(out_info.has_audio, "audio stream should be copied");
        let d = out_info.duration.as_secs_f64();
        assert!((0.5..=1.5).contains(&d), "unexpected duration {d}");

        let decoded = Command::new(&ffmpeg_exe)
            .arg("-i")
            .arg(&output)
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .output()
            .expect("decode output");
        assert!(decoded.status.success());
        let px = &decoded.stdout;
        assert_eq!(px.len(), 24 * 32 * 4);
        let (r, g, b) = (px[0], px[1], px[2]);
        assert!(
            r > 200 && g < 60 && b < 60,
            "expected red frame, got ({r},{g},{b})"
        );

        let modified = dir.join("modified.mp4");
        let data = ExportData {
            source: ExportSource::Video(VideoExportInfo {
                path: input.clone(),
                frame_count: info.frame_count,
                duration: info.duration,
            }),
            width: info.width,
            height: info.height,
            modifiers: vec![Modifier::new(ModifierKind::Exposure(Exposure {
                exposure: -10.0,
            }))],
            crop: None,
            rotation: 0,
        };
        do_export(data, &modified, |_| {}).expect("export with modifier");
        let decoded = Command::new(&ffmpeg_exe)
            .arg("-i")
            .arg(&modified)
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .output()
            .expect("decode modified output");
        assert!(decoded.status.success());
        let px = &decoded.stdout;
        assert_eq!(px.len(), 64 * 48 * 4);
        assert!(
            px.chunks_exact(4)
                .all(|p| p[0] < 40 && p[1] < 40 && p[2] < 40),
            "exposure -10 should crush the frame to near-black"
        );
    }
}
