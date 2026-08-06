//! Shared GPU test harness.
//!
//! GPU tests need a real adapter. When none exists they skip, which keeps
//! `cargo test` usable on headless machines but means a green run does not by
//! itself prove the GPU paths were exercised. Set `BLOOM_REQUIRE_GPU=1` to turn
//! a missing adapter into a failure — CI uses it so the goldens cannot silently
//! stop running.

use iced::wgpu::{
    Device, DeviceDescriptor, Instance, PowerPreference, Queue, RequestAdapterOptions,
};

/// Serializes GPU tests. Several tests each build a device and submit work;
/// running them concurrently can exhaust smaller adapters.
pub(crate) static GPU_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquires a device, or `None` when the machine has no usable adapter.
///
/// Panics instead of returning `None` when `BLOOM_REQUIRE_GPU=1`, so an
/// environment that is supposed to have a GPU cannot skip its coverage quietly.
pub(crate) fn try_device() -> Option<(Device, Queue)> {
    guard_skip(request_device())
}

/// Applies the require-GPU policy to an acquisition result.
///
/// Split from [`try_device`] so the policy is testable without needing a
/// machine that genuinely lacks an adapter.
fn guard_skip(device: Option<(Device, Queue)>) -> Option<(Device, Queue)> {
    if device.is_none() && require_gpu() {
        panic!(
            "BLOOM_REQUIRE_GPU=1 but no wgpu adapter could be acquired; \
             GPU tests would have skipped silently"
        );
    }
    device
}

fn require_gpu() -> bool {
    std::env::var("BLOOM_REQUIRE_GPU").is_ok_and(|v| v == "1")
}

fn request_device() -> Option<(Device, Queue)> {
    let instance = Instance::default();
    let adapter = futures::executor::block_on(instance.request_adapter(&RequestAdapterOptions {
        power_preference: PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .ok()?;
    futures::executor::block_on(adapter.request_device(&DeviceDescriptor::default())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The env var is process-global, so the two tests that mutate it must not
    /// interleave with each other.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard(Option<String>);

    impl EnvGuard {
        fn set(value: Option<&str>) -> Self {
            let prev = std::env::var("BLOOM_REQUIRE_GPU").ok();
            match value {
                Some(v) => unsafe { std::env::set_var("BLOOM_REQUIRE_GPU", v) },
                None => unsafe { std::env::remove_var("BLOOM_REQUIRE_GPU") },
            }
            Self(prev)
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(v) => unsafe { std::env::set_var("BLOOM_REQUIRE_GPU", v) },
                None => unsafe { std::env::remove_var("BLOOM_REQUIRE_GPU") },
            }
        }
    }

    #[test]
    fn missing_adapter_panics_when_gpu_is_required() {
        let _serialize = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = EnvGuard::set(Some("1"));
        let err = std::panic::catch_unwind(|| guard_skip(None)).unwrap_err();
        let msg = err
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or_default();
        assert!(
            msg.contains("BLOOM_REQUIRE_GPU"),
            "panic should name the flag that caused it, got: {msg}"
        );
    }

    #[test]
    fn missing_adapter_skips_when_gpu_is_not_required() {
        let _serialize = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = EnvGuard::set(None);
        assert!(
            guard_skip(None).is_none(),
            "headless machines must still be able to run the suite"
        );
    }

    #[test]
    fn require_gpu_flag_reads_env() {
        let _serialize = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Guards the contract that only an explicit "1" escalates a skip into a
        // failure; a stray empty value must not fail an otherwise fine machine.
        {
            let _env = EnvGuard::set(Some("1"));
            assert!(require_gpu());
        }
        {
            let _env = EnvGuard::set(Some("0"));
            assert!(!require_gpu());
        }
        {
            let _env = EnvGuard::set(None);
            assert!(!require_gpu());
        }
    }
}
