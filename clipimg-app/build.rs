fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=resource.rc");
        println!("cargo:rerun-if-changed=icons/icon.ico");
        embed_resource::compile("resource.rc", embed_resource::NONE);
    }
}
