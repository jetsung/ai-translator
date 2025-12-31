use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::debug;

/// 翻译记录器 - 跟踪已翻译和失败的文件
#[derive(Debug)]
pub struct TranslationRecorder {
    translated_file: PathBuf,
    failed_file: PathBuf,
    translated_set: Arc<Mutex<HashSet<String>>>,
    failed_set: Arc<Mutex<HashSet<String>>>,
    translated_writer: Arc<Mutex<File>>,
    failed_writer: Arc<Mutex<File>>,
}

impl TranslationRecorder {
    /// 创建新的翻译记录器
    pub fn new(translated_file: impl AsRef<Path>, failed_file: impl AsRef<Path>) -> Result<Self> {
        let translated_file = translated_file.as_ref().to_path_buf();
        let failed_file = failed_file.as_ref().to_path_buf();

        // 创建父目录
        if let Some(parent) = translated_file.parent() {
            fs::create_dir_all(parent).with_context(|| format!("无法创建目录: {:?}", parent))?;
        }
        if let Some(parent) = failed_file.parent() {
            fs::create_dir_all(parent).with_context(|| format!("无法创建目录: {:?}", parent))?;
        }

        // 加载已存在的记录
        let translated_set = Self::load_set(&translated_file)?;
        let failed_set = Self::load_set(&failed_file)?;

        // 打开文件用于追加写入
        let translated_writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&translated_file)
            .with_context(|| format!("无法打开文件: {:?}", translated_file))?;

        let failed_writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&failed_file)
            .with_context(|| format!("无法打开文件: {:?}", failed_file))?;

        Ok(Self {
            translated_file,
            failed_file,
            translated_set: Arc::new(Mutex::new(translated_set)),
            failed_set: Arc::new(Mutex::new(failed_set)),
            translated_writer: Arc::new(Mutex::new(translated_writer)),
            failed_writer: Arc::new(Mutex::new(failed_writer)),
        })
    }

    /// 从文件加载路径集合
    fn load_set(file_path: &Path) -> Result<HashSet<String>> {
        let mut set = HashSet::new();

        if file_path.exists() {
            let file =
                File::open(file_path).with_context(|| format!("无法打开文件: {:?}", file_path))?;

            for path in std::io::BufReader::new(file).lines().map_while(Result::ok) {
                let path = path.trim();
                if !path.is_empty() {
                    set.insert(path.to_string());
                }
            }
        }

        Ok(set)
    }

    /// 检查文件是否已翻译
    pub fn is_translated(&self, abs_path: &Path) -> bool {
        let path_str = abs_path.to_string_lossy().to_string();
        self.translated_set
            .lock()
            .map(|set| set.contains(&path_str))
            .unwrap_or(false)
    }

    /// 记录翻译成功
    pub fn record_success(&self, abs_path: &Path) -> Result<()> {
        let path_str = abs_path.to_string_lossy().to_string();

        // 添加到已翻译集合
        {
            let mut set = self
                .translated_set
                .lock()
                .map_err(|e| anyhow::anyhow!("获取已翻译集合锁失败: {}", e))?;

            if !set.contains(&path_str) {
                set.insert(path_str.clone());

                // 写入文件
                let mut writer = self
                    .translated_writer
                    .lock()
                    .map_err(|e| anyhow::anyhow!("获取已翻译文件写入锁失败: {}", e))?;

                writeln!(writer, "{}", path_str)
                    .with_context(|| format!("写入已翻译文件失败: {:?}", self.translated_file))?;
            }
        }

        // 从失败集合中移除
        self.remove_from_failed(&path_str)?;

        Ok(())
    }

    /// 记录翻译失败
    pub fn record_failure(&self, abs_path: &Path) -> Result<()> {
        let path_str = abs_path.to_string_lossy().to_string();

        // 如果已经在已翻译集合中，则不记录失败
        if self.is_translated(abs_path) {
            return Ok(());
        }

        let mut set = self
            .failed_set
            .lock()
            .map_err(|e| anyhow::anyhow!("获取失败集合锁失败: {}", e))?;

        if !set.contains(&path_str) {
            set.insert(path_str.clone());

            // 写入文件
            let mut writer = self
                .failed_writer
                .lock()
                .map_err(|e| anyhow::anyhow!("获取失败文件写入锁失败: {}", e))?;

            writeln!(writer, "{}", path_str)
                .with_context(|| format!("写入失败文件失败: {:?}", self.failed_file))?;
        }

        Ok(())
    }

    /// 从失败集合中移除
    fn remove_from_failed(&self, path_str: &str) -> Result<()> {
        let mut set = self
            .failed_set
            .lock()
            .map_err(|e| anyhow::anyhow!("获取失败集合锁失败: {}", e))?;

        if set.contains(path_str) {
            set.remove(path_str);

            // 重写失败文件
            drop(set); // 释放锁

            let writer = self
                .failed_writer
                .lock()
                .map_err(|e| anyhow::anyhow!("获取失败文件写入锁失败: {}", e))?;

            // 关闭当前文件句柄
            drop(writer);

            // 重写文件
            let set = self
                .failed_set
                .lock()
                .map_err(|e| anyhow::anyhow!("获取失败集合锁失败: {}", e))?;

            let mut file = File::create(&self.failed_file)
                .with_context(|| format!("无法创建文件: {:?}", self.failed_file))?;

            for p in set.iter() {
                writeln!(file, "{}", p)
                    .with_context(|| format!("写入失败文件失败: {:?}", self.failed_file))?;
            }

            // 重新打开追加模式
            let new_writer = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.failed_file)
                .with_context(|| format!("无法打开文件: {:?}", self.failed_file))?;

            // 替换 writer
            let mut writer_guard = self
                .failed_writer
                .lock()
                .map_err(|e| anyhow::anyhow!("获取失败文件写入锁失败: {}", e))?;
            *writer_guard = new_writer;
        }

        Ok(())
    }

    /// 获取失败文件列表
    pub fn get_failed_files(&self) -> Vec<String> {
        self.failed_set
            .lock()
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }
}

impl Drop for TranslationRecorder {
    fn drop(&mut self) {
        debug!("TranslationRecorder 正在关闭");
    }
}
