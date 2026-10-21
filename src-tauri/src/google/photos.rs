#![allow(dead_code)]

use super::types::{AlbumItem, GoogleError, PhotoItem, SemanticPhotoQuery};
use regex::Regex;
use serde_json::Value;

/// Google Photos domain service.
pub struct PhotosService;

impl PhotosService {
    /// Parse Google Photos API mediaItems response into `Vec<PhotoItem>`.
    pub fn parse_media_items_json(json: &Value) -> Result<Vec<PhotoItem>, GoogleError> {
        let items = json
            .get("mediaItems")
            .and_then(|v| v.as_array())
            .ok_or_else(|| GoogleError::SerializationError("Missing mediaItems array in response".into()))?;

        let mut photos = Vec::new();

        for item in items {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let filename = item.get("filename").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let product_url = item.get("productUrl").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let mime_type = item.get("mimeType").and_then(|v| v.as_str()).unwrap_or("image/jpeg").to_string();
            let creation_time_iso = item
                .pointer("/mediaMetadata/creationTime")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let description = item.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());

            photos.push(PhotoItem {
                id,
                filename,
                product_url,
                mime_type,
                creation_time_iso,
                description,
            });
        }

        Ok(photos)
    }

    /// Parse Google Photos albums list response into `Vec<AlbumItem>`.
    pub fn parse_albums_json(json: &Value) -> Result<Vec<AlbumItem>, GoogleError> {
        let items = json
            .get("albums")
            .and_then(|v| v.as_array())
            .ok_or_else(|| GoogleError::SerializationError("Missing albums array in response".into()))?;

        let mut albums = Vec::new();

        for item in items {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("(Untitled)").to_string();
            let total_media_items = item
                .get("mediaItemsCount")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            albums.push(AlbumItem {
                id,
                title,
                total_media_items,
            });
        }

        Ok(albums)
    }

    /// Parse natural voice speech into structured `SemanticPhotoQuery`.
    /// e.g. "show photos of dogs from July 2024", "pictures of food", "vacation photos in Paris"
    pub fn parse_semantic_query(speech: &str) -> SemanticPhotoQuery {
        let clean = speech.trim().to_lowercase();

        // Detect category
        let category = if clean.contains("dog") || clean.contains("cat") || clean.contains("pet") || clean.contains("animal") {
            Some("ANIMALS".to_string())
        } else if clean.contains("food") || clean.contains("dish") || clean.contains("dinner") || clean.contains("lunch") {
            Some("FOOD".to_string())
        } else if clean.contains("landmark") || clean.contains("monument") || clean.contains("tower") {
            Some("LANDMARKS".to_string())
        } else if clean.contains("receipt") || clean.contains("document") || clean.contains("bill") {
            Some("DOCUMENTS".to_string())
        } else if clean.contains("selfie") || clean.contains("people") || clean.contains("family") {
            Some("PEOPLE".to_string())
        } else {
            None
        };

        // Extract year if present (e.g. 2023, 2024, 2025, 2026)
        let year_re = Regex::new(r"\b(20[2-3][0-9])\b").ok();
        let year = year_re.and_then(|re| re.find(&clean).map(|m| m.as_str().to_string()));

        let (date_from, date_to) = if let Some(ref y) = year {
            (Some(format!("{}-01-01", y)), Some(format!("{}-12-31", y)))
        } else {
            (None, None)
        };

        // Strip boilerplate phrases
        let query_text = clean
            .replace("show photos of", "")
            .replace("show pictures of", "")
            .replace("find photos of", "")
            .replace("photos of", "")
            .replace("pictures of", "")
            .replace("photos from", "")
            .replace("pictures from", "")
            .replace("photos in", "")
            .replace("pictures in", "")
            .trim()
            .to_string();

        SemanticPhotoQuery {
            query: if query_text.is_empty() { clean } else { query_text },
            date_from,
            date_to,
            category,
        }
    }

    /// Build JSON payload for mediaItems:search Google Photos API.
    pub fn build_search_payload(query: &SemanticPhotoQuery, page_size: u32) -> Value {
        let mut filters = serde_json::Map::new();

        if let Some(ref cat) = query.category {
            filters.insert(
                "contentFilter".into(),
                serde_json::json!({
                    "includedContentCategories": [cat]
                }),
            );
        }

        if let (Some(ref from), Some(ref to)) = (&query.date_from, &query.date_to) {
            let from_parts: Vec<&str> = from.split('-').collect();
            let to_parts: Vec<&str> = to.split('-').collect();
            if from_parts.len() == 3 && to_parts.len() == 3 {
                filters.insert(
                    "dateFilter".into(),
                    serde_json::json!({
                        "ranges": [{
                            "startDate": {
                                "year": from_parts[0].parse::<u32>().unwrap_or(2026),
                                "month": from_parts[1].parse::<u32>().unwrap_or(1),
                                "day": from_parts[2].parse::<u32>().unwrap_or(1)
                            },
                            "endDate": {
                                "year": to_parts[0].parse::<u32>().unwrap_or(2026),
                                "month": to_parts[1].parse::<u32>().unwrap_or(12),
                                "day": to_parts[2].parse::<u32>().unwrap_or(31)
                            }
                        }]
                    }),
                );
            }
        }

        serde_json::json!({
            "pageSize": page_size,
            "filters": filters
        })
    }

    /// Append dimension parameters to baseUrl for high-res or thumbnail display.
    /// e.g. baseUrl=w800-h600 or =w256-h256-c (cropped)
    pub fn get_sized_base_url(base_url: &str, width: u32, height: u32, crop: bool) -> String {
        let crop_suffix = if crop { "-c" } else { "" };
        format!("{}=w{}-h{}{}", base_url, width, height, crop_suffix)
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_media_items_json() {
        let raw = serde_json::json!({
            "mediaItems": [
                {
                    "id": "photo_101",
                    "filename": "sunset.jpg",
                    "productUrl": "https://photos.google.com/photo/101",
                    "mimeType": "image/jpeg",
                    "mediaMetadata": {
                        "creationTime": "2026-08-15T19:42:00Z"
                    },
                    "description": "Sunset at the beach"
                }
            ]
        });

        let items = PhotosService::parse_media_items_json(&raw).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "photo_101");
        assert_eq!(items[0].filename, "sunset.jpg");
        assert_eq!(items[0].creation_time_iso, "2026-08-15T19:42:00Z");
        assert_eq!(items[0].description, Some("Sunset at the beach".to_string()));
    }

    #[test]
    fn test_semantic_photo_query_parser() {
        let q = PhotosService::parse_semantic_query("show pictures of my dog in 2024");
        assert_eq!(q.category, Some("ANIMALS".to_string()));
        assert_eq!(q.date_from, Some("2024-01-01".to_string()));
        assert_eq!(q.date_to, Some("2024-12-31".to_string()));

        let q2 = PhotosService::parse_semantic_query("photos of food");
        assert_eq!(q2.category, Some("FOOD".to_string()));
        assert_eq!(q2.date_from, None);
    }

    #[test]
    fn test_build_search_payload() {
        let query = SemanticPhotoQuery {
            query: "food".into(),
            date_from: Some("2025-01-01".into()),
            date_to: Some("2025-12-31".into()),
            category: Some("FOOD".into()),
        };

        let payload = PhotosService::build_search_payload(&query, 25);
        assert_eq!(payload["pageSize"], 25);
        assert_eq!(payload["filters"]["contentFilter"]["includedContentCategories"][0], "FOOD");
        assert_eq!(payload["filters"]["dateFilter"]["ranges"][0]["startDate"]["year"], 2025);
    }

    #[test]
    fn test_parse_albums_json() {
        let raw = serde_json::json!({
            "albums": [
                {
                    "id": "alb_999",
                    "title": "Summer Trip",
                    "mediaItemsCount": "42"
                }
            ]
        });

        let albums = PhotosService::parse_albums_json(&raw).unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].id, "alb_999");
        assert_eq!(albums[0].title, "Summer Trip");
        assert_eq!(albums[0].total_media_items, 42);
    }

    #[test]
    fn test_get_sized_base_url() {
        let base = "https://lh3.googleusercontent.com/xyz123";
        let thumb = PhotosService::get_sized_base_url(base, 256, 256, true);
        assert_eq!(thumb, "https://lh3.googleusercontent.com/xyz123=w256-h256-c");

        let large = PhotosService::get_sized_base_url(base, 1920, 1080, false);
        assert_eq!(large, "https://lh3.googleusercontent.com/xyz123=w1920-h1080");
    }
}
