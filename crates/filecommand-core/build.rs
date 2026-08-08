//! `libgit2-sys`'s Windows/MSVC build script always compiles libgit2's
//! Windows-specific process-token / registry / CryptoAPI object files
//! (`fs_path.c`'s owner checks, `sysdir.c`'s registry lookups, `rand.c`'s
//! `CryptAcquireContextA` seed), but does not itself declare `advapi32` as
//! a link dependency for them — only `winhttp`/`rpcrt4`/`ole32`/`crypt32`/
//! `secur32` (see `target/debug/build/libgit2-sys-*/output`). Without it,
//! linking anything that pulls in `git2` (the `git_info` capability, M5
//! §3.1) fails with `LNK2019` on `OpenProcessToken`, `RegOpenKeyExW`,
//! `CryptCreateHash`, and friends. Declare it explicitly so a plain `cargo
//! build`/`cargo test` succeeds without every downstream crate having to
//! carry this workaround (crate README "Build requirements").

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=advapi32");
    }
}
