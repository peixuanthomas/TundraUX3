fn main() {
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let exit_code = tundra_cli::run(std::env::args().skip(1), &mut stdout, &mut stderr);

    std::process::exit(exit_code);
}
