fn main() {
    #[cfg(target_os = "linux")]
    if let Err(error) = tundra_privileged::linux::run() {
        eprintln!("tundra-privileged: {error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("tundra-privileged is only available on Linux");
        std::process::exit(1);
    }
}
