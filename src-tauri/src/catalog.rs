// dsh-launcher · 签名 runtime catalog(M1:供应链可信基础)
//
// catalog.json(canonical 字节)+ catalog.json.sig(Ed25519)随 Launcher 发布;
// 本模块内置公钥并验签,签名失败属于安全失败(拒绝加载,绝不忽略)。
// catalog 固定 component/version/platform/URL/size/SHA-256;所有 URL 均为国内镜像
// (npmmirror),catalog 是唯一下载来源,下载失败不静默回退境外源。
//
// 重新签名(仅维护者,私钥在仓库外 ~/.dsh-launcher-catalog-signing.key):
//   cargo run --example catalog_tool -- sign src-tauri/resources/catalog.json \
//     ~/.dsh-launcher-catalog-signing.key src-tauri/resources/catalog.json.sig
use serde::{Deserialize, Serialize};

pub const CATALOG_SCHEMA: u32 = 1;

/// 内置公钥(hex)。对应私钥不在仓库内。
pub const CATALOG_PUBKEY_HEX: &str =
    "31475d4099455813e5bda1980995daeb5a39791c5a8bb24cf072059741544784";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub version: String,
    pub platform: String,
    pub kind: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Catalog {
    pub schema: u32,
    pub generated_at: String,
    #[serde(default)]
    pub note: String,
    pub components: Vec<CatalogEntry>,
}

/// 随应用发布的 catalog 原始字节(验签对象,严禁格式化改写)。
pub fn catalog_bytes() -> &'static [u8] {
    include_bytes!("../resources/catalog.json")
}

/// 随应用发布的 Ed25519 签名(64 字节)。
pub fn catalog_signature() -> &'static [u8] {
    include_bytes!("../resources/catalog.json.sig")
}

/// 用内置公钥验证 catalog 字节签名。
pub fn verify_embedded() -> Result<(), String> {
    verify(catalog_bytes(), catalog_signature(), CATALOG_PUBKEY_HEX)
}

/// 通用验签(内置 catalog 自检 + 未来外部 catalog 加载共用)。
pub fn verify(bytes: &[u8], sig: &[u8], pubkey_hex: &str) -> Result<(), String> {
    use ed25519_dalek::{Signature, VerifyingKey};
    let pk_bytes = hex::decode(pubkey_hex).map_err(|e| format!("公钥 hex 非法:{e}"))?;
    let pk: VerifyingKey = VerifyingKey::from_bytes(
        pk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "公钥长度非法".to_string())?,
    )
    .map_err(|e| format!("公钥非法:{e}"))?;
    let sig = Signature::from_slice(sig).map_err(|e| format!("签名非法:{e}"))?;
    pk.verify_strict(bytes, &sig)
        .map_err(|e| format!("catalog 签名校验失败(安全失败):{e}"))
}

/// 加载并验证内置 catalog。验签失败返回 Err(安全失败)。
pub fn load_catalog() -> Result<Catalog, String> {
    verify_embedded()?;
    let cat: Catalog =
        serde_json::from_slice(catalog_bytes()).map_err(|e| format!("catalog 解析失败:{e}"))?;
    if cat.schema != CATALOG_SCHEMA {
        return Err(format!("catalog schema 不兼容:{}", cat.schema));
    }
    for c in &cat.components {
        if c.sha256.len() != 64 || hex::decode(&c.sha256).is_err() {
            return Err(format!("catalog 组件 {} 的 sha256 非法", c.id));
        }
    }
    Ok(cat)
}

/// 按 id + platform 查找组件(platform 精确匹配,`any` 匹配任意)。
pub fn lookup<'a>(
    cat: &'a Catalog,
    id: &str,
    version: &str,
    platform: &str,
) -> Option<&'a CatalogEntry> {
    cat.components.iter().find(|c| {
        c.id == id && c.version == version && (c.platform == platform || c.platform == "any")
    })
}

/// 当前平台标识(win-x64 / darwin-arm64 / darwin-x64 / linux-x64…)。
pub fn current_platform() -> String {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "macos" if arch == "aarch64" => "darwin-arm64".into(),
        "macos" => "darwin-x64".into(),
        "windows" if arch == "x86_64" => "win-x64".into(),
        "windows" => format!("win-{arch}"),
        "linux" if arch == "aarch64" => "linux-arm64".into(),
        "linux" => "linux-x64".into(),
        other => format!("{other}-{arch}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_verifies_and_parses() {
        let cat = load_catalog().expect("内置 catalog 必须能验签并解析");
        assert_eq!(cat.schema, CATALOG_SCHEMA);
        assert!(!cat.components.is_empty());
        // 必需组件
        assert!(lookup(&cat, "node", "v24.9.0", "win-x64").is_some());
        assert!(lookup(&cat, "node", "v24.9.0", "darwin-arm64").is_some());
        assert!(lookup(&cat, "mingit", "2.55.0.4", "win-x64").is_some());
        assert!(lookup(&cat, "pnpm", "11.7.0", "any").is_some());
    }

    #[test]
    fn tampered_catalog_fails_closed() {
        let mut bytes = catalog_bytes().to_vec();
        let n = bytes.len();
        // 篡改一个字符
        bytes[n / 2] ^= 0x01;
        let err = verify(&bytes, catalog_signature(), CATALOG_PUBKEY_HEX);
        assert!(err.is_err(), "篡改后必须验签失败");
        let msg = err.unwrap_err();
        assert!(msg.contains("安全失败"), "{msg}");
    }

    #[test]
    fn tampered_signature_fails_closed() {
        let mut sig = catalog_signature().to_vec();
        sig[10] ^= 0x01;
        assert!(verify(catalog_bytes(), &sig, CATALOG_PUBKEY_HEX).is_err());
    }

    #[test]
    fn wrong_pubkey_fails() {
        let bad = "0".repeat(64);
        assert!(verify(catalog_bytes(), catalog_signature(), &bad).is_err());
    }

    #[test]
    fn all_urls_are_domestic_mirrors() {
        let cat = load_catalog().unwrap();
        for c in &cat.components {
            assert!(
                c.url.starts_with("https://registry.npmmirror.com/"),
                "catalog 组件 URL 必须为国内镜像: {}",
                c.url
            );
            assert_eq!(hex::decode(&c.sha256).unwrap().len(), 32);
        }
    }

    #[test]
    fn current_platform_shape() {
        let p = current_platform();
        assert!(
            p.contains("darwin") || p.contains("win") || p.contains("linux"),
            "{p}"
        );
    }
}
