fn main() {
    println!("cargo:rerun-if-changed=assets/mado-screen.rc");
    println!("cargo:rerun-if-changed=assets/mado-screen.ico");
    // Fail rather than silently ship a Windows executable without its icon.
    // https://docs.rs/embed-resource/3.0.12/embed_resource/enum.CompilationResult.html
    embed_resource::compile("assets/mado-screen.rc", embed_resource::NONE)
        .manifest_required()
        .expect("compile executable icon");
}
