#[cfg(target_os = "linux")]
mod display;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod pam;
#[cfg(target_os = "linux")]
mod process;
#[cfg(target_os = "linux")]
mod runtime;
fn main() {
    #[cfg(target_os = "linux")]
    if let Err(error) = linux::run() {
        eprintln!("tundra-sessiond: {error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("tundra-sessiond is available only on Linux");
        std::process::exit(1);
    }
}
