fn main() {
    #[cfg(windows)]
    winres::WindowsResource::new()
        .set_icon("assets/logo/bloom.ico")
        .compile()
        .expect("failed to compile Windows resource");
}
