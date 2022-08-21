fn main() {
    // If the input capnp file is not in the current directory then the output file will not be
    // created directly in $OUT_DIR, but rather in some subdirectory. Let's avoid that.
    std::env::set_current_dir(std::env::current_dir().unwrap().parent().unwrap()).unwrap();

    println!(
        "cargo:rerun-if-changed={}",
        std::env::current_dir()
            .unwrap()
            .join("bearclaw.capnp")
            .display()
    );

    capnpc::CompilerCommand::new()
        .file("bearclaw.capnp")
        .run()
        .expect("compiling bearclaw.capnp");

    std::env::set_current_dir(std::env::current_dir().unwrap().join("bearclaw-proxy")).unwrap();

    let mut config = vergen::Config::default();
    *config.git_mut().semver_dirty_mut() = Some("-dirty");
    vergen::vergen(config).unwrap();
}
