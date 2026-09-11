fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(not(target_os = "linux"))]
fn run() -> system_maintenance::Result<()> {
    Err(system_maintenance::invalid(
        "system maintenance is Linux-only",
    ))
}
#[cfg(target_os = "linux")]
fn run() -> system_maintenance::Result<()> {
    use system_maintenance::{invalid, linux, migration};
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, id] if command == "prepare" => {
            println!(
                "{}",
                serde_json::to_string(&linux::prepare_official_release(id)?)?
            );
            Ok(())
        }
        [command, id] if command == "apply" || command == "apply-prepared" => {
            linux::apply_prepared(id)
        }
        [command, id] if command == "restore-labels" => linux::restore_labels(id),
        [command] if command == "recover" => linux::recover(),
        [command] if command == "__import" => migration::import_stdin(),
        [command, tail @ ..] if command == "migrate-legacy" => {
            let mut source = None;
            let mut uid = None;
            let mut apply = false;
            let mut iter = tail.iter();
            while let Some(arg) = iter.next() {
                match arg.as_str() {
                    "--source" if source.is_none() => source = iter.next(),
                    "--uid" if uid.is_none() => {
                        uid = Some(iter.next().ok_or_else(|| invalid("missing UID"))?.parse()?)
                    }
                    "--apply" if !apply => apply = true,
                    _ => {
                        return Err(invalid(
                            "usage: migrate-legacy --source PATH --uid UID [--apply]",
                        ));
                    }
                }
            }
            let report = migration::run(
                std::path::Path::new(source.ok_or_else(|| invalid("--source required"))?),
                uid.ok_or_else(|| invalid("--uid required"))?,
                apply,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        _ => Err(invalid(
            "usage: tundra-system-maintenance prepare RELEASE | apply RELEASE | recover | restore-labels RELEASE | migrate-legacy --source PATH --uid UID [--apply]",
        )),
    }
}
