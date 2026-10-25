use std::{env, fs, path::Path};

fn collect(root: &Path, directory: &Path, output: &mut String) {
    let mut entries = fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, output);
        } else if path.extension().is_some_and(|extension| extension == "sh") {
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let text = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
            output.push_str(&format!("({name:?}, {text:?}),\n"));
        }
    }
}

fn main() {
    let root = Path::new("../../third_party/bashdb");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut output = String::from("static BASHDB_FILES: &[(&str, &str)] = &[\n");
    for directory in ["init", "lib", "command"] {
        collect(root, &root.join(directory), &mut output);
    }
    for name in ["dbg-main.sh", "getopts_long.sh", "set-d-vars.sh", "COPYING"] {
        let text = fs::read_to_string(root.join(name))
            .unwrap()
            .replace("\r\n", "\n");
        output.push_str(&format!("({name:?}, {text:?}),\n"));
    }
    let text = fs::read_to_string(root.join("bashdb-trace.in"))
        .unwrap()
        .replace("\r\n", "\n")
        .replace("@PACKAGE@", "bashdb")
        .replace("@SH_PROG@", "/bin/bash")
        .replace("@prefix@", "/")
        .replace("@PKGDATADIR@", "/nonexistent/tundra-bashdb");
    output.push_str(&format!("(\"bashdb-trace\", {text:?}),\n];\n"));
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("bundle.rs"),
        output,
    )
    .unwrap();
}
