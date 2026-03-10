use anyhow::Result;
use rmcp::{Error as McpError, model::Content};

use crate::mcp::types::{FileReferenceAttachment, ImageAttachment, McpResponse, McpResponseContent};

/// 解析 MCP 响应内容
///
/// 支持新的结构化格式和旧格式的兼容性，并生成适当的 Content 对象
pub fn parse_mcp_response(response: &str) -> Result<Vec<Content>, McpError> {
    if response.trim() == "CANCELLED" || response.trim() == "用户取消了操作" {
        return Ok(vec![Content::text("用户取消了操作".to_string())]);
    }

    // 首先尝试解析为新的结构化格式
    if let Ok(structured_response) = serde_json::from_str::<McpResponse>(response) {
        return parse_structured_response(structured_response);
    }

    // 回退到旧格式兼容性解析
    match serde_json::from_str::<Vec<McpResponseContent>>(response) {
        Ok(content_array) => {
            let mut result = Vec::new();
            let mut image_count = 0;

            // 分别收集用户文本和图片信息
            let mut user_text_parts = Vec::new();
            let mut image_info_parts = Vec::new();

            for content in content_array {
                match content.content_type.as_str() {
                    "text" => {
                        if let Some(text) = content.text {
                            user_text_parts.push(text);
                        }
                    }
                    "image" => {
                        if let Some(source) = content.source {
                            if source.source_type == "base64" {
                                image_count += 1;

                                // 先添加图片到结果中（图片在前）
                                result.push(Content::image(source.data.clone(), source.media_type.clone()));

                                // 添加图片信息到图片信息部分
                                let base64_len = source.data.len();
                                let preview = if base64_len > 50 {
                                    format!("{}...", &source.data[..50])
                                } else {
                                    source.data.clone()
                                };

                                // 计算图片大小（base64解码后的大小）
                                let estimated_size = (base64_len * 3) / 4; // base64编码后大约增加33%
                                let size_str = if estimated_size < 1024 {
                                    format!("{} B", estimated_size)
                                } else if estimated_size < 1024 * 1024 {
                                    format!("{:.1} KB", estimated_size as f64 / 1024.0)
                                } else {
                                    format!("{:.1} MB", estimated_size as f64 / (1024.0 * 1024.0))
                                };

                                let image_info = format!(
                                    "=== 图片 {} ===\n类型: {}\n大小: {}\nBase64 预览: {}\n完整 Base64 长度: {} 字符",
                                    image_count, source.media_type, size_str, preview, base64_len
                                );
                                image_info_parts.push(image_info);
                            }
                        }
                    }
                    _ => {
                        // 未知类型，作为文本处理
                        if let Some(text) = content.text {
                            user_text_parts.push(text);
                        }
                    }
                }
            }

            // 构建文本内容：用户文本 + 图片信息 + 注意事项
            let mut all_text_parts = Vec::new();

            // 1. 用户输入的文本
            if !user_text_parts.is_empty() {
                all_text_parts.extend(user_text_parts);
            }

            // 2. 图片详细信息
            if !image_info_parts.is_empty() {
                all_text_parts.extend(image_info_parts);
            }

            // 3. 兼容性说明
            if image_count > 0 {
                all_text_parts.push(format!(
                    "💡 注意：用户提供了 {} 张图片。如果 AI 助手无法显示图片，图片数据已包含在上述 Base64 信息中。",
                    image_count
                ));
            }

            // 将所有文本内容合并并添加到结果末尾（图片后面）
            if !all_text_parts.is_empty() {
                let combined_text = all_text_parts.join("\n\n");
                result.push(Content::text(combined_text));
            }

            if result.is_empty() {
                result.push(Content::text("用户未提供任何内容".to_string()));
            }

            Ok(result)
        }
        Err(_) => {
            // 如果不是JSON格式，作为纯文本处理
            Ok(vec![Content::text(response.to_string())])
        }
    }
}

/// 解析新的结构化响应格式
fn parse_structured_response(response: McpResponse) -> Result<Vec<Content>, McpError> {
    let mut result = Vec::new();
    for image in &response.images {
        result.push(Content::image(image.data.clone(), image.media_type.clone()));
    }

    let combined_text = build_structured_context_text(&response);
    if !combined_text.is_empty() {
        result.push(Content::text(combined_text));
    }

    if result.is_empty() {
        result.push(Content::text("用户未提供任何内容".to_string()));
    }

    Ok(result)
}

fn build_structured_context_text(response: &McpResponse) -> String {
    let mut sections = Vec::new();
    let mut user_message_parts = Vec::new();
    let mut preference_lines = Vec::new();

    if let Some(user_input) = response.user_input.as_ref() {
        let (user_message, preferences) = split_user_message_and_preferences(user_input);
        if !user_message.is_empty() {
            user_message_parts.push(user_message);
        }
        preference_lines.extend(preferences);
    }

    if !user_message_parts.is_empty() {
        sections.push(user_message_parts.join("\n"));
    }

    let mut context_lines = Vec::new();

    if !response.selected_options.is_empty() {
        let selected_options = response.selected_options
            .iter()
            .map(|option| serde_json::to_string(option).unwrap_or_else(|_| "\"\"".to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        context_lines.push(format!("- 选项: [{}]", selected_options));
    }

    if !response.files.is_empty() {
        let references = response.files
            .iter()
            .enumerate()
            .map(|(index, file)| format!("- 资源{}: {}", index + 1, format_file_reference(file)))
            .collect::<Vec<_>>();
        context_lines.extend(references);
    }

    if !response.images.is_empty() {
        let images = response.images
            .iter()
            .enumerate()
            .map(|(index, image)| format!("- 图片{}: {}", index + 1, format_image_attachment(image)))
            .collect::<Vec<_>>();
        context_lines.extend(images);
    }

    if !context_lines.is_empty() {
        sections.push(format!(
            "附加上下文：\n{}",
            context_lines.join("\n")
        ));
    }

    if !preference_lines.is_empty() {
        sections.push(format!(
            "执行偏好：\n{}",
            preference_lines
                .into_iter()
                .map(|line| format!("- {}", line))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    sections.join("\n\n")
}

fn split_user_message_and_preferences(user_input: &str) -> (String, Vec<String>) {
    let mut message_lines = Vec::new();
    let mut preference_lines = Vec::new();

    for line in user_input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !message_lines.is_empty() {
                message_lines.push(String::new());
            }
            continue;
        }

        if is_preference_line(trimmed) {
            preference_lines.push(trimmed.to_string());
        } else {
            message_lines.push(trimmed.to_string());
        }
    }

    while message_lines.last().is_some_and(|line| line.is_empty()) {
        message_lines.pop();
    }

    (message_lines.join("\n"), preference_lines)
}

fn is_preference_line(line: &str) -> bool {
    line.starts_with('✔') || line.starts_with('❌')
}

fn format_file_reference(file: &FileReferenceAttachment) -> String {
    let reference_kind = if file.r#type == "url" {
        "url"
    } else if file.kind.as_deref() == Some("directory") {
        "directory"
    } else {
        "file"
    };

    let location = if file.r#type == "url" {
        file.url.as_deref().unwrap_or_default()
    } else {
        file.path.as_deref().unwrap_or_default()
    };

    let mut fields = vec![
        format!("type: {}", serde_json::to_string(reference_kind).unwrap_or_else(|_| "\"\"".to_string())),
        format!("name: {}", serde_json::to_string(&file.name).unwrap_or_else(|_| "\"\"".to_string())),
    ];

    if file.r#type == "url" {
        fields.push(format!("url: {}", serde_json::to_string(location).unwrap_or_else(|_| "\"\"".to_string())));
    } else {
        fields.push(format!("path: {}", serde_json::to_string(location).unwrap_or_else(|_| "\"\"".to_string())));
    }

    if let Some(mime_type) = file.mime_type.as_ref() {
        fields.push(format!("mime_type: {}", serde_json::to_string(mime_type).unwrap_or_else(|_| "\"\"".to_string())));
    }

    format!("{{ {} }}", fields.join(", "))
}

fn format_image_attachment(image: &ImageAttachment) -> String {
    let base64_len = image.data.len();
    let estimated_size = (base64_len * 3) / 4;
    let size_str = if estimated_size < 1024 {
        format!("{} B", estimated_size)
    } else if estimated_size < 1024 * 1024 {
        format!("{:.1} KB", estimated_size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", estimated_size as f64 / (1024.0 * 1024.0))
    };

    let mut fields = vec![
        format!("media_type: {}", serde_json::to_string(&image.media_type).unwrap_or_else(|_| "\"\"".to_string())),
        format!("size: {}", serde_json::to_string(&size_str).unwrap_or_else(|_| "\"\"".to_string())),
        format!("base64: {}", serde_json::to_string(&image.data).unwrap_or_else(|_| "\"\"".to_string())),
    ];

    if let Some(filename) = image.filename.as_ref() {
        fields.push(format!("filename: {}", serde_json::to_string(filename).unwrap_or_else(|_| "\"\"".to_string())));
    }

    format!("{{ {} }}", fields.join(", "))
}
