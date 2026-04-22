use std::path::Path;
use std::sync::Arc;

use agentix::tool;
use serde_json::json;

pub struct UiSpells {
    pub sandbox: Arc<crate::sandbox::SandboxManager>,
    pub user_id: uuid::Uuid,
    pub conversation_id: uuid::Uuid,
}

#[tool]
impl Tool for UiSpells {
    /// 将文件展示给用户（在 UI 中渲染为一个类似 Claude 的文件卡片，支持预览和下载）。
    /// 适合展示生成的图表、导出的数据、或者需要用户重点关注的代码文件。
    /// 只能展示 /workspace 下的文件；如果文件不在 /workspace/public 下，会自动复制过去。
    ///
    /// description: 本次展示的简短说明（例如："我为你生成了数据分析报告"）
    /// path: 文件的完整路径（以 /workspace/ 开头）
    async fn present_file(&self, description: Option<String>, path: String) -> serde_json::Value {
        let _ = description;
        let q_path = std::path::PathBuf::from(&path);
        if !q_path.starts_with("/workspace") {
            return json!({ "error": "path must be under /workspace" });
        }
        let conv_dir = self
            .sandbox
            .get_conversation_dir(self.user_id, self.conversation_id);
        let public_dir = conv_dir.join("public");
        let relative = q_path.strip_prefix("/workspace").unwrap();
        let host_path = conv_dir.join(relative);

        let filename = host_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        let meta = tokio::fs::metadata(&host_path).await;
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);

        // Directories → zip into public/
        if is_dir {
            let zip_filename = format!("{}.zip", filename);
            let zip_host_path = public_dir.join(&zip_filename);
            let _ = tokio::fs::create_dir_all(&public_dir).await;
            if let Err(e) = zip_dir(&host_path, &zip_host_path) {
                return json!({ "error": format!("打包失败: {e}") });
            }
            let size = tokio::fs::metadata(&zip_host_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            return json!({
                "display": "file",
                "filename": zip_filename,
                "path": format!("/workspace/public/{}", zip_filename),
                "size": size
            });
        }

        // File already in public → use as-is
        if q_path.starts_with("/workspace/public") {
            let size = meta.map(|m| m.len()).unwrap_or(0);
            return json!({
                "display": "file",
                "filename": filename,
                "path": path,
                "size": size
            });
        }

        // File outside public → copy into public/
        let _ = tokio::fs::create_dir_all(&public_dir).await;
        let dest_host = public_dir.join(&filename);
        if let Err(e) = tokio::fs::copy(&host_path, &dest_host).await {
            return json!({ "error": format!("复制到 public 失败: {e}") });
        }
        let size = tokio::fs::metadata(&dest_host)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        json!({
            "display": "file",
            "filename": filename,
            "path": format!("/workspace/public/{}", filename),
            "size": size
        })
    }

    /// 向用户提问并等待回答后再继续。
    /// 适合需要确认、选择或补充信息的场景。
    /// 有 options 时前端渲染为快捷按钮。
    /// 调用后当前生成会自动结束，用户回答将作为新消息继续对话。
    ///
    /// description: 本次操作意图（供 UI 渲染，可不填）
    /// question: 向用户展示的问题文本，必须简洁明了
    /// options: 供用户选择的选项（可选）
    async fn ask(
        &self,
        description: Option<String>,
        question: String,
        options: Option<Vec<String>>,
    ) -> serde_json::Value {
        // Return a marker that the Worker will detect to pause generation.
        // The user's answer will arrive as a normal new message.
        json!({
            "__ask__": true,
            "description": description,
            "question": question,
            "options": options,
        })
    }

    /// 在对话中内嵌渲染一个交互式 widget（图表、可视化、计算器等）。
    /// 适合需要交互、动画、或复杂数据可视化的场景（滑块、点击、Chart.js、D3 等）。
    /// 不需要交互时请优先用 diagram 工具，速度更快。
    ///
    /// title: snake_case 标识符，唯一标识这个 widget（如 q4_revenue_chart）
    /// loading_messages: 1-4 条 loading 提示语，streaming 期间循环展示（如 ["绘制坐标轴...", "填充数据..."]）
    /// widget_code: 完整的 HTML 代码片段（可使用 Chart.js、D3 等 CDN 库）。
    ///   不要包含 <!DOCTYPE>、<html>、<head>、<body> 标签，直接写内容。
    ///   可使用 CSS 变量：--text-primary、--bg-surface、--accent 等与 familiar 主题一致。
    async fn visualize(
        &self,
        title: Option<String>,
        loading_messages: Option<Vec<String>>,
        widget_code: String,
    ) -> serde_json::Value {
        let _ = (title, loading_messages);
        json!({ "status": "success" })
    }

    /// 在对话中内嵌渲染一个 Mermaid 图表（流程图、时序图、ER 图、甘特图等）。
    /// 适合不需要交互的静态图表，生成速度极快。
    /// 需要交互或复杂动画时请用 visualize 工具。
    ///
    /// 支持的图表类型：flowchart、sequenceDiagram、classDiagram、erDiagram、
    ///   gantt、pie、gitgraph、mindmap、timeline 等所有 Mermaid 语法。
    /// code: 合法的 Mermaid 图表代码，不要包含 markdown 代码块标记。
    async fn diagram(&self, code: String) -> serde_json::Value {
        let _ = code;
        json!({ "status": "success" })
    }
}

fn zip_dir(src: &Path, dst: &Path) -> anyhow::Result<()> {
    use std::fs::File;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    let file = File::create(dst)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let entry_path = entry.path();
        let relative = entry_path.strip_prefix(src)?;
        if entry_path.is_dir() {
            if relative.as_os_str().is_empty() {
                continue;
            }
            zip.add_directory(relative.to_string_lossy(), options)?;
        } else {
            zip.start_file(relative.to_string_lossy(), options)?;
            let mut f = std::fs::File::open(entry_path)?;
            std::io::copy(&mut f, &mut zip)?;
        }
    }
    zip.finish()?;
    Ok(())
}
