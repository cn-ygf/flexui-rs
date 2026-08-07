//! flexui-resource：统一资源抽象（L2/L3）。对应需求 RM1-5。
//!
//! 核心是「按逻辑路径拿字节」的 `ResourceProvider`，可叠多层（先命中者优先）：
//! - `DirProvider`：从文件系统目录读（RM1，类 duilib SetResourcePath）
//! - `ZipProvider`：从**带密码 zip** 读，来源可为磁盘文件（RM2）或内嵌字节（RM3）
//! - 使用者用 `ResourceManager::read` 统一读取任意文件（RM4）；XML/图片/字体都走它（RM5）。

use std::io::{BufReader, Cursor, Read, Seek};
use std::path::PathBuf;

use zip::ZipArchive;

/// 资源读取错误。
#[derive(Debug)]
pub enum ResError {
    /// 逻辑路径非法（越权 `..` 等）。
    BadPath(String),
    /// 未找到。
    NotFound(String),
    /// IO 错误。
    Io(String),
    /// 压缩包错误（含密码错误）。
    Zip(String),
    /// 非 UTF-8 文本。
    Utf8(String),
}

impl std::fmt::Display for ResError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResError::BadPath(p) => write!(f, "非法资源路径: {p}"),
            ResError::NotFound(p) => write!(f, "资源未找到: {p}"),
            ResError::Io(e) => write!(f, "资源 IO 错误: {e}"),
            ResError::Zip(e) => write!(f, "压缩包错误: {e}"),
            ResError::Utf8(e) => write!(f, "资源非 UTF-8: {e}"),
        }
    }
}
impl std::error::Error for ResError {}

impl From<std::io::Error> for ResError {
    fn from(e: std::io::Error) -> Self {
        ResError::Io(e.to_string())
    }
}
impl From<zip::result::ZipError> for ResError {
    fn from(e: zip::result::ZipError) -> Self {
        ResError::Zip(e.to_string())
    }
}

/// 规范化逻辑路径：去掉 `res://` 前缀与 `./`，统一正斜杠，拒绝 `..` 越权。
pub fn normalize(path: &str) -> Result<String, ResError> {
    let p = path.strip_prefix("res://").unwrap_or(path);
    let p = p.replace('\\', "/");
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => return Err(ResError::BadPath(path.to_string())),
            s => out.push(s),
        }
    }
    Ok(out.join("/"))
}

/// 资源提供者：把逻辑路径解析成字节。
pub trait ResourceProvider {
    fn read(&self, path: &str) -> Result<Vec<u8>, ResError>;
    fn exists(&self, path: &str) -> bool {
        self.read(path).is_ok()
    }
}

/// 资源管理器：叠多层 provider，read 时按挂载顺序取第一个命中（先挂优先，便于覆盖/换肤）。
#[derive(Default)]
pub struct ResourceManager {
    providers: Vec<Box<dyn ResourceProvider>>,
}

impl ResourceManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一层 provider。
    pub fn mount(&mut self, p: impl ResourceProvider + 'static) -> &mut Self {
        self.providers.push(Box::new(p));
        self
    }

    /// 读取任意文件字节。
    pub fn read(&self, path: &str) -> Result<Vec<u8>, ResError> {
        let path = normalize(path)?;
        for p in &self.providers {
            if let Ok(b) = p.read(&path) {
                return Ok(b);
            }
        }
        Err(ResError::NotFound(path))
    }

    /// 读取为 UTF-8 文本（XML 等）。
    pub fn read_string(&self, path: &str) -> Result<String, ResError> {
        let bytes = self.read(path)?;
        String::from_utf8(bytes).map_err(|e| ResError::Utf8(e.to_string()))
    }

    /// 是否存在。
    pub fn exists(&self, path: &str) -> bool {
        normalize(path)
            .ok()
            .map(|p| self.providers.iter().any(|pr| pr.exists(&p)))
            .unwrap_or(false)
    }
}

/// 目录 provider（RM1）。
pub struct DirProvider {
    base: PathBuf,
}

impl DirProvider {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }
}

impl ResourceProvider for DirProvider {
    fn read(&self, path: &str) -> Result<Vec<u8>, ResError> {
        let full = self.base.join(path);
        std::fs::read(&full).map_err(|_| ResError::NotFound(path.to_string()))
    }
    fn exists(&self, path: &str) -> bool {
        self.base.join(path).is_file()
    }
}

/// zip 数据来源。
enum ZipSource {
    File(PathBuf),
    Bytes(Vec<u8>),
}

/// 带密码 zip provider：来源为磁盘文件（RM2）或内嵌字节（RM3）。
pub struct ZipProvider {
    source: ZipSource,
    password: Option<String>,
}

impl ZipProvider {
    /// 从磁盘 zip 文件读（带密码）。
    pub fn open(path: impl Into<PathBuf>, password: impl Into<String>) -> Self {
        Self {
            source: ZipSource::File(path.into()),
            password: Some(password.into()),
        }
    }
    /// 从磁盘 zip 文件读（无密码）。
    pub fn open_plain(path: impl Into<PathBuf>) -> Self {
        Self {
            source: ZipSource::File(path.into()),
            password: None,
        }
    }
    /// 从内嵌字节读（RM3；配合 `include_bytes!`）。
    pub fn embedded(bytes: impl Into<Vec<u8>>, password: impl Into<String>) -> Self {
        Self {
            source: ZipSource::Bytes(bytes.into()),
            password: Some(password.into()),
        }
    }
    /// 从内嵌字节读（无密码）。
    pub fn embedded_plain(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            source: ZipSource::Bytes(bytes.into()),
            password: None,
        }
    }
}

impl ResourceProvider for ZipProvider {
    fn read(&self, path: &str) -> Result<Vec<u8>, ResError> {
        // 每次读重新打开（无内部可变状态；skin 体量小成本可接受）。
        match &self.source {
            ZipSource::File(p) => {
                let file = std::fs::File::open(p)?;
                extract(ZipArchive::new(BufReader::new(file))?, path, self.password.as_deref())
            }
            ZipSource::Bytes(b) => {
                extract(ZipArchive::new(Cursor::new(b.clone()))?, path, self.password.as_deref())
            }
        }
    }
}

/// 从已打开的 zip 归档解出某文件字节。
fn extract<R: Read + Seek>(
    mut archive: ZipArchive<R>,
    path: &str,
    password: Option<&str>,
) -> Result<Vec<u8>, ResError> {
    let mut buf = Vec::new();
    match password {
        Some(pwd) => {
            let mut f = archive.by_name_decrypt(path, pwd.as_bytes())?;
            f.read_to_end(&mut buf)?;
        }
        None => {
            let mut f = archive.by_name(path)?;
            f.read_to_end(&mut buf)?;
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::FileOptions;
    use zip::AesMode;

    /// 造一个含 `skin/main.xml` 的 AES 加密 zip。
    fn make_encrypted_zip(password: &str, content: &[u8]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut cursor);
            let opts: FileOptions<()> =
                FileOptions::default().with_aes_encryption(AesMode::Aes256, password);
            w.start_file("skin/main.xml", opts).unwrap();
            w.write_all(content).unwrap();
            w.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn 路径规范化() {
        assert_eq!(normalize("res://a/b.xml").unwrap(), "a/b.xml");
        assert_eq!(normalize("./a/./b.png").unwrap(), "a/b.png");
        assert_eq!(normalize("a\\b\\c").unwrap(), "a/b/c");
        assert!(normalize("../secret").is_err());
    }

    #[test]
    fn 内嵌带密码_zip_读取() {
        let bytes = make_encrypted_zip("p@ss", b"<VBox/>");
        let mut rm = ResourceManager::new();
        rm.mount(ZipProvider::embedded(bytes, "p@ss"));
        assert_eq!(rm.read_string("skin/main.xml").unwrap(), "<VBox/>");
        assert!(rm.exists("skin/main.xml"));
        assert!(rm.read("skin/none.xml").is_err());
    }

    #[test]
    fn 目录_provider_读取() {
        let dir = std::env::temp_dir().join(format!("flexui_res_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("skin")).unwrap();
        std::fs::write(dir.join("skin/a.txt"), b"hello").unwrap();
        let mut rm = ResourceManager::new();
        rm.mount(DirProvider::new(&dir));
        assert_eq!(rm.read_string("skin/a.txt").unwrap(), "hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 叠层先挂优先() {
        let zbytes = make_encrypted_zip("k", b"FROM_ZIP");
        let dir = std::env::temp_dir().join(format!("flexui_ov_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("skin")).unwrap();
        std::fs::write(dir.join("skin/main.xml"), b"FROM_DIR").unwrap();
        let mut rm = ResourceManager::new();
        rm.mount(ZipProvider::embedded(zbytes, "k")); // 先挂 → 优先
        rm.mount(DirProvider::new(&dir));
        assert_eq!(rm.read_string("skin/main.xml").unwrap(), "FROM_ZIP");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
