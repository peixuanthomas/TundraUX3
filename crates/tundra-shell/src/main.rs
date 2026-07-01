#[cfg(not(windows))]
compile_error!("TundraUX3 phase 0 supports Windows 11 only.");

fn main() {
    let mut stdout = std::io::stdout();

    let result = match tundra_shell::parse_shell_args(std::env::args().skip(1)) {
        Ok(tundra_shell::ShellLaunchMode::Fullscreen) => {
            tundra_shell::run_fullscreen_blocking(&mut stdout)
        }
        Ok(tundra_shell::ShellLaunchMode::NotFullscreen) => {
            tundra_shell::run_not_fullscreen(&mut stdout)
        }
        Err(error) => {
            eprintln!("tundra-shell failed: {error}");
            std::process::exit(2);
        }
    };

    if let Err(error) = result {
        eprintln!("tundra-shell failed: {error}");
        std::process::exit(1);
    }
}
