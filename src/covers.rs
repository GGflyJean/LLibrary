use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

#[derive(Debug, Clone)]
pub struct CoverStore {
    dir: PathBuf,
    pdftoppm: PathBuf,
}

impl CoverStore {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("无法创建封面目录 {}", dir.display()))?;
        Ok(Self {
            dir,
            pdftoppm: PathBuf::from("pdftoppm"),
        })
    }

    #[cfg(test)]
    pub fn with_pdftoppm(mut self, binary: impl Into<PathBuf>) -> Self {
        self.pdftoppm = binary.into();
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for(&self, work_id: i64) -> PathBuf {
        self.dir.join(format!("{work_id}.jpg"))
    }

    pub fn url_for(work_id: i64) -> String {
        format!("/covers/{work_id}.jpg")
    }

    pub fn ready_marker(&self, work_id: i64) -> PathBuf {
        self.dir.join(format!("{work_id}.ready"))
    }

    pub fn is_ready(&self, work_id: i64) -> bool {
        self.ready_marker(work_id).exists()
    }

    pub fn mark_ready(&self, work_id: i64) -> Result<()> {
        std::fs::write(self.ready_marker(work_id), b"1")?;
        Ok(())
    }

    pub fn save_epub_cover(&self, work_id: i64, bytes: &[u8]) -> Result<Option<String>> {
        let Ok(image) = image::load_from_memory(bytes) else {
            return Ok(None);
        };
        let thumbnail = image.thumbnail(400, 400);
        let path = self.path_for(work_id);
        thumbnail
            .save_with_format(&path, ImageFormat::Jpeg)
            .with_context(|| format!("无法保存封面 {}", path.display()))?;
        let _ = self.mark_ready(work_id);
        Ok(Some(Self::url_for(work_id)))
    }

    pub fn ensure_placeholder(&self, work_id: i64) -> Result<String> {
        let path = self.path_for(work_id);
        if path.exists() {
            return Ok(Self::url_for(work_id));
        }
        let hue = (work_id.unsigned_abs() % 6) as u8;
        let color = match hue {
            0 => Rgb([76, 110, 145]),
            1 => Rgb([110, 90, 130]),
            2 => Rgb([90, 120, 100]),
            3 => Rgb([140, 105, 80]),
            4 => Rgb([100, 100, 120]),
            _ => Rgb([120, 80, 90]),
        };
        let mut image = RgbImage::from_pixel(400, 600, color);
        for x in 0..400u32 {
            for y in 0..600u32 {
                if x < 4 || y < 4 || x >= 396 || y >= 596 {
                    image.put_pixel(x, y, Rgb([235, 235, 235]));
                }
            }
        }
        DynamicImage::ImageRgb8(image)
            .save_with_format(&path, ImageFormat::Jpeg)
            .with_context(|| format!("无法保存占位封面 {}", path.display()))?;
        Ok(Self::url_for(work_id))
    }

    pub fn save_pdf_cover(&self, work_id: i64, pdf_bytes: &[u8]) -> Result<String> {
        match self.try_render_pdf(pdf_bytes, work_id) {
            Ok(path) => {
                let _ = self.mark_ready(work_id);
                Ok(path)
            }
            Err(error) => {
                tracing::warn!(work_id, %error, "PDF 封面生成失败，使用占位图");
                let path = self.ensure_placeholder(work_id)?;
                let _ = self.mark_ready(work_id);
                Ok(path)
            }
        }
    }

    fn try_render_pdf(&self, pdf_bytes: &[u8], work_id: i64) -> Result<String> {
        let unique = format!(
            "llibrary-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let temp_dir = std::env::temp_dir();
        let input = temp_dir.join(format!("{unique}.pdf"));
        let prefix = temp_dir.join(&unique);
        std::fs::write(&input, pdf_bytes)?;

        let output = Command::new(&self.pdftoppm)
            .args([
                "-f",
                "1",
                "-l",
                "1",
                "-scale-to",
                "400",
                "-jpeg",
                "-singlefile",
            ])
            .arg(&input)
            .arg(&prefix)
            .output();

        let result = match output {
            Ok(output) if output.status.success() => {
                let rendered = prefix.with_extension("jpg");
                let data = std::fs::read(&rendered)
                    .with_context(|| format!("无法读取 pdftoppm 输出 {}", rendered.display()))?;
                let image = image::load_from_memory(&data)?;
                let thumbnail = image.thumbnail(400, 400);
                let path = self.path_for(work_id);
                thumbnail.save_with_format(&path, ImageFormat::Jpeg)?;
                let _ = std::fs::remove_file(&rendered);
                Ok(Self::url_for(work_id))
            }
            Ok(output) => Err(anyhow::anyhow!(
                "pdftoppm 退出码 {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )),
            Err(error) => Err(anyhow::anyhow!("pdftoppm 不可用: {error}")),
        };

        let _ = std::fs::remove_file(&input);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn png_bytes() -> Vec<u8> {
        let image: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(20, 30, Rgba([200, 30, 40, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn saves_epub_cover() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CoverStore::new(dir.path()).expect("store");
        let path = store
            .save_epub_cover(7, &png_bytes())
            .expect("save")
            .expect("path");
        assert_eq!(path, "/covers/7.jpg");
        assert!(store.path_for(7).exists());
    }

    #[test]
    fn invalid_cover_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CoverStore::new(dir.path()).expect("store");
        assert!(store
            .save_epub_cover(1, b"not an image")
            .expect("save")
            .is_none());
    }

    #[test]
    fn placeholder_is_created() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CoverStore::new(dir.path()).expect("store");
        let path = store.ensure_placeholder(3).expect("placeholder");
        assert_eq!(path, "/covers/3.jpg");
        assert!(store.path_for(3).exists());
    }

    #[test]
    fn missing_pdftoppm_falls_back_to_placeholder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = CoverStore::new(dir.path()).expect("store");
        // If pdftoppm exists, this corrup input still fails and falls back.
        let path = store.save_pdf_cover(9, b"not a pdf").expect("cover");
        assert_eq!(path, "/covers/9.jpg");
        assert!(store.path_for(9).exists());
    }
}

#[cfg(test)]
mod success_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn renders_pdf_cover_with_stub_binary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let jpeg = dir.path().join("sample.jpg");
        let image: image::RgbImage = image::RgbImage::from_pixel(20, 30, Rgb([20, 40, 60]));
        DynamicImage::ImageRgb8(image)
            .save_with_format(&jpeg, ImageFormat::Jpeg)
            .expect("jpeg");

        let script = dir.path().join("pdftoppm-stub.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor last; do :; done\ncp '{}' \"${{last}}.jpg\"\n",
                jpeg.display()
            ),
        )
        .expect("script");
        let mut permissions = std::fs::metadata(&script).expect("meta").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("chmod");

        let store = CoverStore::new(dir.path().join("covers"))
            .expect("store")
            .with_pdftoppm(script);
        let path = store.save_pdf_cover(42, b"fake pdf").expect("cover");
        assert_eq!(path, "/covers/42.jpg");
        assert!(store.path_for(42).exists());
        assert!(store.is_ready(42));
    }
}
