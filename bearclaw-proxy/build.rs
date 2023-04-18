use std::io::Write;

const THIRD_PARTY_LICENSES_FILENAME: &str = "THIRD-PARTY-LICENSES";
const SQL_DIRECTORY: &str = "sql";
const SQL_FILENAME: &str = "create.sql";
const CAPNP_FILENAME: &str = "bearclaw.capnp";
const OUTPUT_LICENSES_FILENAME: &str = "licenses.txt";

fn main() {
    let this_project_dir = std::env::current_dir().unwrap();
    let root_project_dir = this_project_dir
        .parent()
        .map(std::path::PathBuf::from)
        .unwrap();
    let out_dir = std::env::var("OUT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap();

    eprintln!("this_project_dir: {this_project_dir:?}");
    eprintln!("root_project_dir: {root_project_dir:?}");
    eprintln!("out_dir: {out_dir:?}");

    // Rebuild if SQL files are changed
    println!(
        "cargo:rerun-if-changed={}",
        this_project_dir
            .join(SQL_DIRECTORY)
            .join(SQL_FILENAME)
            .display()
    );

    // If the input capnp file is not in the current directory then the output file will not be
    // created directly in $OUT_DIR, but rather in some subdirectory. Let's avoid that.
    std::env::set_current_dir(&root_project_dir).unwrap();

    {
        // Process the Cap'n Proto IDL files

        println!(
            "cargo:rerun-if-changed={}",
            root_project_dir.join(CAPNP_FILENAME).display()
        );

        capnpc::CompilerCommand::new()
            .file(CAPNP_FILENAME)
            .run()
            .unwrap_or_else(|_| panic!("Failed to compile {CAPNP_FILENAME}"));

        // Generate third-party license information

        println!(
            "cargo:rerun-if-changed={}",
            root_project_dir
                .join(THIRD_PARTY_LICENSES_FILENAME)
                .display()
        );

        let output = std::process::Command::new("cargo")
            .arg("license")
            .arg("-j")
            .output()
            .expect("Please run: cargo install cargo-license");

        assert!(
            output.status.success(),
            ">>>>>>>>>> Please run: cargo install cargo-license <<<<<<<<<<"
        );

        let static_libraries = output.stdout;
        let dynamic_libraries =
            std::fs::read_to_string(root_project_dir.join(THIRD_PARTY_LICENSES_FILENAME))
                .unwrap_or_else(|_| panic!("Could not read {THIRD_PARTY_LICENSES_FILENAME} file"));
        let mut out =
            std::fs::File::create(out_dir.join(OUTPUT_LICENSES_FILENAME)).unwrap_or_else(|_| {
                panic!("Could not create a new file '{OUTPUT_LICENSES_FILENAME}' in '{out_dir:?}'")
            });

        writeln!(
            out,
            "This project uses the following third-party static libraries"
        )
        .unwrap();
        writeln!(out, "(in alphabetical order):").unwrap();
        writeln!(out).unwrap();
        out.write_all(&static_libraries).unwrap();
        writeln!(out).unwrap();
        write!(out, "{dynamic_libraries}").unwrap();

        out.flush().unwrap();
    }

    std::env::set_current_dir(&this_project_dir).unwrap();

    // Generate build version information

    let mut config = vergen::Config::default();
    *config.git_mut().semver_dirty_mut() = Some("-dirty");
    *config.git_mut().skip_if_error_mut() = true;
    vergen::vergen(config).unwrap();
}
