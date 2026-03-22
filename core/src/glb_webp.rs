//! GLB WebP texture converter.
//!
//! Converts GLB files with WebP textures to use PNG textures instead,
//! making them compatible with loaders that don't support the
//! EXT_texture_webp extension (e.g., three-d-asset).

use crate::constants::http::mime;
use image::ImageFormat;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

/// Convert a GLB file with WebP textures to use PNG textures instead.
///
/// This function:
/// 1. Parses the GLB file to extract GLTF JSON and binary buffers
/// 2. Identifies images using WebP format (via EXT_texture_webp)
/// 3. Decodes WebP images and re-encodes as PNG
/// 4. Updates the GLTF JSON to remove WebP extension and use PNG
/// 5. Returns the modified GLB as bytes
pub fn convert_webp_to_png(glb_path: &Path) -> Result<Vec<u8>, String> {
    // Read the GLB file
    let glb_data =
        std::fs::read(glb_path).map_err(|e| format!("Failed to read GLB file: {}", e))?;

    // Parse the GLB file using gltf crate
    let glb =
        gltf::Glb::from_slice(&glb_data).map_err(|e| format!("Failed to parse GLB: {}", e))?;

    // Parse the GLTF JSON
    let gltf_json: serde_json::Value = serde_json::from_slice(&glb.json)
        .map_err(|e| format!("Failed to parse GLTF JSON: {}", e))?;

    // Check if this GLB uses EXT_texture_webp
    let uses_webp = gltf_json
        .get("extensionsUsed")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|v| v.as_str() == Some("EXT_texture_webp")))
        .unwrap_or(false);

    // Note: We don't early-return here even if !uses_webp because we still need to
    // add target fields to buffer views for three-d-asset compatibility

    // Always work with cloned JSON and binary data (we'll modify them)
    let mut new_gltf_json = gltf_json.clone();
    let bin_data = glb.bin.as_ref().ok_or("GLB file has no binary buffer")?;
    let mut new_bin_data = bin_data.to_vec();

    // Only do WebP conversion if needed
    if uses_webp {
        let mut image_replacements: HashMap<usize, Vec<u8>> = HashMap::new();

        // Collect buffer view info first (to avoid borrowing issues)
        let buffer_views = new_gltf_json
            .get("bufferViews")
            .and_then(|v| v.as_array())
            .ok_or("Missing bufferViews")?
            .clone(); // Clone to avoid borrow conflicts

        // Process each image in the GLTF
        if let Some(images) = new_gltf_json
            .get_mut("images")
            .and_then(|v| v.as_array_mut())
        {
            for (img_idx, image) in images.iter_mut().enumerate() {
                // Check if this image uses WebP in two possible formats:
                // 1. EXT_texture_webp extension with source field
                // 2. Direct mimeType: image/webp
                let (has_webp, buffer_view_idx) = if let Some(ext_source) = image
                    .get("extensions")
                    .and_then(|v| v.as_object())
                    .and_then(|obj| obj.get("EXT_texture_webp"))
                    .and_then(|ext| ext.get("source"))
                    .and_then(|v| v.as_u64())
                {
                    // Format 1: Extension-based WebP
                    (true, ext_source as usize)
                } else if image.get("mimeType").and_then(|v| v.as_str()) == Some(mime::IMAGE_WEBP) {
                    // Format 2: Direct mimeType WebP
                    let bv_idx = image
                        .get("bufferView")
                        .and_then(|v| v.as_u64())
                        .ok_or("WebP image missing bufferView")?
                        as usize;
                    (true, bv_idx)
                } else {
                    (false, 0)
                };

                if !has_webp {
                    continue;
                }

                // Get the buffer view
                let buffer_view = buffer_views
                    .get(buffer_view_idx)
                    .ok_or("Invalid bufferView index")?;

                let byte_offset = buffer_view
                    .get("byteOffset")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;

                let byte_length = buffer_view
                    .get("byteLength")
                    .and_then(|v| v.as_u64())
                    .ok_or("Missing byteLength")? as usize;

                // Extract WebP image data (with bounds check)
                let end = byte_offset
                    .checked_add(byte_length)
                    .ok_or("Buffer view offset + length overflow")?;
                if end > bin_data.len() {
                    return Err(format!(
                        "Buffer view out of bounds: offset {} + length {} > buffer size {}",
                        byte_offset,
                        byte_length,
                        bin_data.len()
                    ));
                }
                let webp_data = &bin_data[byte_offset..end];

                // Decode WebP image
                let img = image::load_from_memory_with_format(webp_data, ImageFormat::WebP)
                    .map_err(|e| format!("Failed to decode WebP image {}: {}", img_idx, e))?;

                // Re-encode as RGBA PNG (force alpha channel for OpenGL compatibility —
                // some GPU drivers mishandle 3-byte-per-pixel RGB textures due to alignment)
                let rgba_img = img.to_rgba8();
                let mut png_data = Vec::new();
                let mut cursor = Cursor::new(&mut png_data);
                rgba_img
                    .write_to(&mut cursor, ImageFormat::Png)
                    .map_err(|e| format!("Failed to encode PNG: {}", e))?;

                // Store the replacement
                image_replacements.insert(img_idx, png_data);

                // Update the JSON to remove EXT_texture_webp and add standard properties
                if let Some(image_obj) = image.as_object_mut() {
                    image_obj.remove("extensions");
                    image_obj.insert("mimeType".to_string(), serde_json::json!(mime::IMAGE_PNG));
                }
            }
        }

        // If we have image replacements, rebuild the binary buffer
        if !image_replacements.is_empty() {
            // Append new PNG images to the end of the buffer and create new buffer views
            let mut offset = new_bin_data.len();
            let mut new_buffer_views = Vec::new();
            let mut image_to_buffer_view: HashMap<usize, usize> = HashMap::new();

            // Calculate initial buffer view count
            let initial_buffer_view_count = new_gltf_json
                .get("bufferViews")
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);

            // Process each replacement: append PNG data and create buffer view info
            for (img_idx, png_data) in &image_replacements {
                // Append PNG data to buffer
                new_bin_data.extend_from_slice(png_data);

                // Create new buffer view
                let new_buffer_view = serde_json::json!({
                    "buffer": 0,
                    "byteOffset": offset,
                    "byteLength": png_data.len(),
                });

                let new_buffer_view_idx = initial_buffer_view_count + new_buffer_views.len();
                new_buffer_views.push(new_buffer_view);
                image_to_buffer_view.insert(*img_idx, new_buffer_view_idx);

                offset += png_data.len();
            }

            // Add new buffer views to the JSON
            if let Some(buffer_views) = new_gltf_json
                .get_mut("bufferViews")
                .and_then(|v| v.as_array_mut())
            {
                for buffer_view in new_buffer_views {
                    buffer_views.push(buffer_view);
                }
            }

            // Update images to reference new buffer views
            if let Some(images) = new_gltf_json
                .get_mut("images")
                .and_then(|v| v.as_array_mut())
            {
                for (img_idx, buffer_view_idx) in image_to_buffer_view {
                    if let Some(image_obj) = images[img_idx].as_object_mut() {
                        image_obj
                            .insert("bufferView".to_string(), serde_json::json!(buffer_view_idx));
                    }
                }
            }

            // Update buffer size in JSON
            if let Some(buffers) = new_gltf_json
                .get_mut("buffers")
                .and_then(|v| v.as_array_mut())
            {
                if let Some(buffer_obj) = buffers.get_mut(0).and_then(|v| v.as_object_mut()) {
                    buffer_obj.insert(
                        "byteLength".to_string(),
                        serde_json::json!(new_bin_data.len()),
                    );
                }
            }

            // Remove EXT_texture_webp from extensionsUsed and extensionsRequired
            if let Some(extensions_used) = new_gltf_json
                .get_mut("extensionsUsed")
                .and_then(|v| v.as_array_mut())
            {
                extensions_used.retain(|v| v.as_str() != Some("EXT_texture_webp"));
            }
            if let Some(extensions_required) = new_gltf_json
                .get_mut("extensionsRequired")
                .and_then(|v| v.as_array_mut())
            {
                extensions_required.retain(|v| v.as_str() != Some("EXT_texture_webp"));
            }
        }
    } // End of if uses_webp

    // Fix textures array - convert EXT_texture_webp to standard source references
    // (This needs to run even if uses_webp is false, because files might have texture
    // extensions without top-level extension declarations)
    if let Some(textures) = new_gltf_json
        .get_mut("textures")
        .and_then(|v| v.as_array_mut())
    {
        for texture in textures.iter_mut() {
            if let Some(texture_obj) = texture.as_object_mut() {
                // Check if this texture uses EXT_texture_webp
                if let Some(ext_source) = texture_obj
                    .get("extensions")
                    .and_then(|v| v.as_object())
                    .and_then(|obj| obj.get("EXT_texture_webp"))
                    .and_then(|ext| ext.get("source"))
                    .and_then(|v| v.as_u64())
                {
                    // Convert to standard texture format
                    texture_obj.remove("extensions");
                    texture_obj.insert("source".to_string(), serde_json::json!(ext_source));
                }
            }
        }
    }

    // Fix buffer views - add "target" field if missing (required by some GLTF loaders)
    // Target values: 34962 = ARRAY_BUFFER (vertex attributes), 34963 = ELEMENT_ARRAY_BUFFER (indices)
    if let Some(accessors) = new_gltf_json
        .get("accessors")
        .and_then(|v| v.as_array())
        .cloned()
    {
        if let Some(buffer_views) = new_gltf_json
            .get_mut("bufferViews")
            .and_then(|v| v.as_array_mut())
        {
            // Track which buffer views are used by which accessors
            let mut buffer_view_usage: HashMap<usize, u32> = HashMap::new();

            for accessor in &accessors {
                if let Some(buffer_view_idx) = accessor.get("bufferView").and_then(|v| v.as_u64()) {
                    let bv_idx = buffer_view_idx as usize;
                    let is_indices =
                        accessor.get("type").and_then(|v| v.as_str()) == Some("SCALAR");
                    let target = if is_indices { 34963 } else { 34962 };
                    buffer_view_usage.insert(bv_idx, target);
                }
            }

            // Apply targets only to buffer views referenced by accessors.
            // Buffer views used for images must NOT get a target field —
            // adding one causes loaders to misinterpret image data as vertex data.
            for (idx, buffer_view) in buffer_views.iter_mut().enumerate() {
                if let Some(bv_obj) = buffer_view.as_object_mut() {
                    if !bv_obj.contains_key("target") {
                        if let Some(&target) = buffer_view_usage.get(&idx) {
                            bv_obj.insert("target".to_string(), serde_json::json!(target));
                        }
                    }
                }
            }
        }
    }

    // Rebuild the GLB file
    let new_json_string = serde_json::to_string(&new_gltf_json)
        .map_err(|e| format!("Failed to serialize JSON: {}", e))?;

    build_glb(&new_json_string, &new_bin_data)
}

/// Build a GLB file from JSON and binary data.
fn build_glb(json: &str, bin: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();

    // GLB header
    output.extend_from_slice(b"glTF"); // magic
    output.extend_from_slice(&2u32.to_le_bytes()); // version

    // Calculate total length
    let header_size = 12;
    let json_chunk_header_size = 8;
    let bin_chunk_header_size = 8;

    // JSON chunk must be padded to 4-byte alignment with spaces
    let json_bytes = json.as_bytes();
    let json_padding = (4 - (json_bytes.len() % 4)) % 4;
    let json_chunk_length = json_bytes.len() + json_padding;

    // BIN chunk must be padded to 4-byte alignment with zeros
    let bin_padding = (4 - (bin.len() % 4)) % 4;
    let bin_chunk_length = bin.len() + bin_padding;

    let total_length = header_size
        + json_chunk_header_size
        + json_chunk_length
        + bin_chunk_header_size
        + bin_chunk_length;

    output.extend_from_slice(&(total_length as u32).to_le_bytes());

    // JSON chunk
    output.extend_from_slice(&(json_chunk_length as u32).to_le_bytes());
    output.extend_from_slice(b"JSON");
    output.extend_from_slice(json_bytes);
    output.extend_from_slice(&vec![b' '; json_padding]); // pad with spaces

    // BIN chunk
    output.extend_from_slice(&(bin_chunk_length as u32).to_le_bytes());
    output.extend_from_slice(b"BIN\0");
    output.extend_from_slice(bin);
    output.extend_from_slice(&vec![0u8; bin_padding]); // pad with zeros

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_glb() {
        let json = r#"{"asset":{"version":"2.0"}}"#;
        let bin = vec![1, 2, 3, 4];

        let result = build_glb(json, &bin);
        assert!(result.is_ok());

        let glb = result.unwrap();
        // Check magic number
        assert_eq!(&glb[0..4], b"glTF");
        // Check version
        assert_eq!(u32::from_le_bytes([glb[4], glb[5], glb[6], glb[7]]), 2);
    }
}
