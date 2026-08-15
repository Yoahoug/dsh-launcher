// dsh-launcher · dev-only catalog 签名工具(不随应用发布)
//
// 用法:
//   cargo run --example catalog_tool -- gen-key <key-out-file>
//      生成 Ed25519 密钥对:私钥 hex 写入 <key-out-file>(请放仓库外,chmod 600),
//      并在 stdout 打印公钥 hex(嵌入 src/catalog.rs 的 CATALOG_PUBKEY_HEX)。
//   cargo run --example catalog_tool -- sign <catalog.json> <key-file> <sig-out>
//      对 catalog.json 原始字节签 Ed25519,签名(64B)写入 <sig-out>。
//   cargo run --example catalog_tool -- verify <catalog.json> <pubkey-hex> <sig-file>
//      校验签名(安全失败)。
//
// 私钥绝不进入 git;catalog 变更必须用同一把私钥重新签名。

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use std::env;
use std::io::Write;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("用法见文件头注释");
        std::process::exit(2);
    }
    match args[1].as_str() {
        "gen-key" => {
            let out = &args[2];
            let mut csprng = OsRng;
            let sk = SigningKey::generate(&mut csprng);
            let pk = sk.verifying_key();
            write_private(out, &sk);
            println!("{}", hex::encode(pk.to_bytes()));
        }
        "sign" => {
            let (catalog, key, sig_out) = (&args[2], &args[3], &args[4]);
            let bytes = std::fs::read(catalog).expect("读取 catalog 失败");
            let sk = read_private(key);
            let sig: Signature = sk.sign(&bytes);
            let sig_bytes = sig.to_bytes();
            std::fs::write(sig_out, sig_bytes).expect("写签名失败");
            println!("签名 {} 字节 → {sig_out}", sig_bytes.len());
        }
        "verify" => {
            let (catalog, pubkey_hex, sig_file) = (&args[2], &args[3], &args[4]);
            let bytes = std::fs::read(catalog).expect("读取 catalog 失败");
            let pk_bytes = hex::decode(pubkey_hex).expect("公钥 hex 非法");
            let pk = VerifyingKey::from_bytes(&pk_bytes.try_into().unwrap()).expect("公钥字节非法");
            let sig_bytes = std::fs::read(sig_file).expect("读取签名失败");
            let sig = Signature::from_slice(&sig_bytes).expect("签名长度非法");
            match pk.verify_strict(&bytes, &sig) {
                Ok(()) => println!("签名有效 ✓"),
                Err(e) => {
                    eprintln!("签名无效:{e}");
                    std::process::exit(1);
                }
            }
        }
        other => {
            eprintln!("未知子命令 {other}");
            std::process::exit(2);
        }
    }
}

fn write_private(path: &str, sk: &SigningKey) {
    let p = Path::new(path);
    if p.exists() {
        eprintln!("{path} 已存在,拒绝覆盖(请手动删除后重试)");
        std::process::exit(3);
    }
    let mut f = std::fs::File::create(p).expect("创建密钥文件失败");
    let mut line = hex::encode(sk.to_bytes());
    line.push('\n');
    f.write_all(line.as_bytes()).expect("写入密钥失败");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
    }
    println!("私钥已写入 {path}(权限 600,请放仓库外)");
}

fn read_private(path: &str) -> SigningKey {
    let raw = std::fs::read_to_string(path).expect("读取密钥失败");
    let hex_str = raw.trim();
    let bytes = hex::decode(hex_str).expect("密钥 hex 非法");
    SigningKey::from_bytes(&bytes.try_into().expect("密钥长度非法"))
}
