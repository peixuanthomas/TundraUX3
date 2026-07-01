fn main() {
    let mut stdout = std::io::stdout();

    let result = match tundra_shell::parse_shell_args(std::env::args().skip(1)) {
        Ok(config) => match config.terminal_mode {
            tundra_shell::ShellTerminalMode::Fullscreen => {
                tundra_shell::run_fullscreen_blocking(&mut stdout, config)
            }
            tundra_shell::ShellTerminalMode::NotFullscreen => {
                tundra_shell::run_not_fullscreen(&mut stdout, config)
            }
        },
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
