// src/main.rs

use actix_files as fs;
use actix_multipart::Multipart;
use actix_web::{
    web, App, HttpResponse, HttpServer, Responder, middleware, Error,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::sync::Arc;
use log::{error, info};
use futures_util::stream::StreamExt;
use std::io::Write;
use uuid::Uuid;
use html_escape::encode_safe; // For HTML escaping
use mime_guess::mime; // Import mime constants for media type detection

// Define supported media types
#[derive(Serialize, Deserialize, Clone)]
enum MediaType {
    Image,
    Video,
}

// Update the Thread struct to include media information
#[derive(Serialize, Deserialize, Clone)]
struct Thread {
    id: i32,
    title: String,
    message: String,
    last_updated: i64, // Unix timestamp
    media_url: Option<String>, // URL to image or video
    media_type: Option<MediaType>, // Type of media: Image or Video
}

// Define Reply struct
#[derive(Serialize, Deserialize)]
struct Reply {
    id: i32,
    message: String,
}

// Define pagination parameters
#[derive(Deserialize)]
struct PaginationParams {
    page: Option<i32>,
}

// Define reply form
#[derive(Deserialize)]
struct ReplyForm {
    parent_id: i32,
    message: String,
}

// Define constants for directories
const IMAGE_UPLOAD_DIR: &str = "./uploads/images/";
const VIDEO_UPLOAD_DIR: &str = "./uploads/videos/";
const IMAGE_THUMB_DIR: &str = "./thumbs/images/";

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize the logger
    env_logger::init();

    // Ensure the uploads and thumbnails directories exist
    for dir in &[IMAGE_UPLOAD_DIR, VIDEO_UPLOAD_DIR, IMAGE_THUMB_DIR] {
        if !std::path::Path::new(dir).exists() {
            std::fs::create_dir_all(dir).unwrap();
            info!("Created directory: {}", dir);
        }
    }

    // Initialize the Sled database
    let sled_db = Arc::new(sled::open("sled_db").expect("Failed to open sled database"));

    // Start the Actix-web server
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(sled_db.clone()))
            .wrap(middleware::Logger::default())
            .service(fs::Files::new("/static", "./static")) // Disabled directory listing
            .service(fs::Files::new("/uploads/images", IMAGE_UPLOAD_DIR)) // Serve uploaded images
            .service(fs::Files::new("/uploads/videos", VIDEO_UPLOAD_DIR)) // Serve uploaded videos
            .service(fs::Files::new("/thumbs/images", IMAGE_THUMB_DIR)) // Serve image thumbnails
            .route("/", web::get().to(homepage))
            .route("/thread/{id}", web::get().to(view_thread))
            .route("/thread", web::post().to(create_thread))
            .route("/reply", web::post().to(create_reply))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}

// Helper function to escape HTML content to prevent XSS
fn escape_html(input: &str) -> String {
    encode_safe(input).to_string()
}

// Helper function to render user-friendly error pages
fn render_error_page(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Error - {}</title>
    <link rel="stylesheet" href="/static/style.css">
</head>
<body>
    <div class="error-container">
        <h1>{}</h1>
        <p>{}</p>
        <a href="/">Back to Home</a>
    </div>
</body>
</html>"#,
        escape_html(title),
        escape_html(title),
        escape_html(message)
    )
}

// Handler for the homepage displaying all threads with pagination
async fn homepage(
    db: web::Data<Arc<Db>>,
    query: web::Query<PaginationParams>,
) -> impl Responder {
    let page_size = 10;
    let page_number = query.page.unwrap_or(1).max(1);

    let mut threads = get_all_threads(&db);
    threads.sort_by(|a, b| b.last_updated.cmp(&a.last_updated));

    let total_threads = threads.len() as i32;
    let total_pages = (total_threads as f64 / page_size as f64).ceil() as i32;

    let page_number = if page_number > total_pages && total_pages > 0 {
        total_pages
    } else {
        page_number
    };

    let start_index = ((page_number - 1) * page_size) as usize;
    let end_index = (start_index + page_size as usize).min(threads.len());
    let threads = &threads[start_index..end_index];

    // Generate HTML for the list of threads
    let thread_list_html = if threads.is_empty() {
        "<p>No threads found. Be the first to create one!</p>".to_string()
    } else {
        threads.iter().map(render_thread).collect::<Vec<String>>().join("<hr>")
    };

    // Generate HTML for pagination controls
    let mut pagination_html = String::new();

    pagination_html.push_str(r#"<div class="pagination">"#);

    if page_number > 1 {
        pagination_html.push_str(&format!(
            r#"<a href="/?page={}">Previous</a>"#,
            page_number - 1
        ));
    }

    for page in 1..=total_pages {
        if page == page_number {
            pagination_html.push_str(&format!(
                r#"<span class="current">{}</span>"#,
                page
            ));
        } else {
            pagination_html.push_str(&format!(
                r#"<a href="/?page={}">{}</a>"#,
                page, page
            ));
        }
    }

    if page_number < total_pages {
        pagination_html.push_str(&format!(
            r#"<a href="/?page={}">Next</a>"#,
            page_number + 1
        ));
    }

    pagination_html.push_str(r#"</div>"#);

    // Assemble the complete HTML for the homepage
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Rust Lang is god!</title>
    <link rel="stylesheet" href="/static/style.css">
    <script defer src="/static/script.js"></script>
</head>
<body>
    <div class="logo">Rust Simple Imageboard 4</div>
    <hr>

    <!-- Create Thread Form -->
    <div id="post-form-container">
        <form class="postform" action="/thread" method="post" enctype="multipart/form-data">
            <input type="text" id="title" name="title" maxlength="75" placeholder="Title" required aria-label="Title">

            <textarea id="message" name="message" rows="4" maxlength="8000" placeholder="Message" required aria-label="Message"></textarea>

            <label for="media">Upload Media (JPEG, PNG, GIF, WEBP, MP4 - optional):</label>
            <input type="file" id="media" name="media" accept=".jpg,.jpeg,.png,.gif,.webp,.mp4">

            <input type="submit" value="Create Thread">
        </form>
    </div>
    <hr>

    <!-- Thread List -->
    <div class="postlists">
        {}
    </div>

    <!-- Pagination Controls -->
    {}

    <div class="footer">
        - Powered by Rust and Actix Web -
    </div>
</body>
</html>"#,
        thread_list_html,
        pagination_html
    );

    HttpResponse::Ok().content_type("text/html").body(html)
}

// Helper function to render individual threads
fn render_thread(thread: &Thread) -> String {
    let media_html = if let (Some(ref url), Some(ref media_type)) = (&thread.media_url, &thread.media_type) {
        match media_type {
            MediaType::Image => {
                // Check if the image is a GIF by its extension
                if url.to_lowercase().ends_with(".gif") {
                    format!(
                        r#"<div class="post-media">
    <img src="{}" alt="Thread Image" class="toggle-image">
</div>"#,
                        escape_html(url)
                    )
                } else {
                    format!(
                        r#"<div class="post-media">
    <img src="{}" alt="Thread Image" class="toggle-image">
</div>"#,
                        escape_html(url)
                    )
                }
            }
            MediaType::Video => format!(
                r#"<div class="post-media">
    <video controls class="video-player">
        <source src="{}" type="video/mp4">
        Your browser does not support the video tag.
    </video>
</div>"#,
                escape_html(url)
            ),
        }
    } else {
        "".to_string()
    };

    format!(
        r#"<div class="post thread-post">
    {}
    <div class="post-content">
        <div class="post-header">
            <span class="title">{}</span>
            <a href="/thread/{}" class="reply-link">Reply</a>
        </div>
        <div class="message">{}</div>
    </div>
</div>"#,
        media_html,
        escape_html(&thread.title),
        thread.id,
        escape_html(&thread.message)
    )
}

// Function to fetch all threads from the Sled database
fn get_all_threads(db: &Db) -> Vec<Thread> {
    db.scan_prefix(b"thread_")
        .filter_map(|res| {
            if let Ok((_, value)) = res {
                serde_json::from_slice(&value).ok()
            } else {
                None
            }
        })
        .collect()
}

// Function to count the total number of threads
fn count_threads(db: &Db) -> i32 {
    db.scan_prefix(b"thread_").count() as i32
}

// Handler to view a specific thread and its replies
async fn view_thread(
    db: web::Data<Arc<Db>>,
    path: web::Path<(i32,)>,
) -> impl Responder {
    let thread_id = path.into_inner().0;
    let thread_key = format!("thread_{}", thread_id).into_bytes();
    let thread: Option<Thread> = db.get(&thread_key).ok().flatten().and_then(|value| {
        serde_json::from_slice(&value).ok()
    });

    if thread.is_none() {
        return HttpResponse::NotFound()
            .content_type("text/html")
            .body(render_error_page("Thread Not Found", "The requested thread does not exist."));
    }

    let thread = thread.unwrap();
    let replies = get_replies(&db, thread_id);

    // Generate HTML for the list of replies
    let replies_html = if replies.is_empty() {
        "<p>No replies yet. Be the first to reply!</p>".to_string()
    } else {
        replies.iter().map(render_reply).collect::<Vec<String>>().join("<hr>")
    };

    // Generate HTML for the thread's media if it exists
    let media_html = if let (Some(ref url), Some(ref media_type)) = (&thread.media_url, &thread.media_type) {
        match media_type {
            MediaType::Image => {
                // Check if the image is a GIF by its extension
                if url.to_lowercase().ends_with(".gif") {
                    format!(
                        r#"<div class="post-media">
    <img src="{}" alt="Thread Image" class="toggle-image">
</div>"#,
                        escape_html(url)
                    )
                } else {
                    format!(
                        r#"<div class="post-media">
    <img src="{}" alt="Thread Image" class="toggle-image">
</div>"#,
                        escape_html(url)
                    )
                }
            }
            MediaType::Video => format!(
                r#"<div class="post-media">
    <video controls class="video-player">
        <source src="{}" type="video/mp4">
        Your browser does not support the video tag.
    </video>
</div>"#,
                escape_html(url)
            ),
        }
    } else {
        "".to_string()
    };

    // Assemble the complete HTML for the thread view
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Thread - {}</title>
    <link rel="stylesheet" href="/static/style.css">
    <script defer src="/static/script.js"></script>
</head>
<body>
    <!-- Reply Mode Label -->
    <div class="replymode">
        <strong>Reply Mode</strong> | <a href="/">Back to Main Board</a>
    </div>
    <br>

    <!-- Reply Form -->
    <div class="postarea-container">
        <form class="postform" action="/reply" method="post">
            <input type="hidden" name="parent_id" value="{}">
            
            <textarea id="message" name="message" rows="4" maxlength="8000" placeholder="Message" required aria-label="Message"></textarea>

            <input type="submit" value="Reply">
        </form>
    </div>
    <br>

    <!-- Main Thread -->
    <div class="post thread-post">
        {}
        <div class="post-content">
            <div class="post-header">
                <span class="title">{}</span>
                <!-- Reply Link Removed -->
            </div>
            <div class="message">{}</div>
        </div>
    </div>
    <hr>

    <!-- Replies -->
    <div class="postlists">
        {}
    </div>
    
    <div class="footer">
        - Powered by Rust and Actix Web -
    </div>
</body>
</html>"#,
        escape_html(&thread.title),
        thread.id,
        media_html,
        escape_html(&thread.title),
        escape_html(&thread.message),
        replies_html
    );

    HttpResponse::Ok().content_type("text/html").body(html)
}

// Helper function to render individual replies
fn render_reply(reply: &Reply) -> String {
    format!(
        r#"<div class="post reply-post">
    <div class="post-content">
        <div class="post-header">
            <span class="title">Reply {}</span>
        </div>
        <div class="message">{}</div>
    </div>
</div>"#,
        reply.id,
        escape_html(&reply.message)
    )
}

// Handler to create a new thread with optional media upload
async fn create_thread(
    db: web::Data<Arc<Db>>,
    mut payload: Multipart,
) -> Result<HttpResponse, Error> {
    let mut title = String::new();
    let mut message = String::new();
    let mut media_url: Option<String> = None;
    let mut media_type: Option<MediaType> = None;

    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_disposition = field.content_disposition();

        let name = if let Some(name) = content_disposition.get_name() {
            name
        } else {
            continue;
        };

        match name {
            "title" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk?;
                    title.push_str(&String::from_utf8_lossy(&data));
                }
            }
            "message" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk?;
                    message.push_str(&String::from_utf8_lossy(&data));
                }
            }
            "media" => {
                // Handle media upload
                if let Some(filename) = content_disposition.get_filename() {
                    // Skip processing if filename is empty
                    if filename.trim().is_empty() {
                        continue;
                    }

                    // Determine the MIME type
                    let mime_type = mime_guess::from_path(&filename).first_or_octet_stream();

                    match mime_type.type_() {
                        mime::IMAGE => {
                            // Supported image subtypes
                            if !matches!(
                                mime_type.subtype().as_ref(),
                                "jpeg" | "png" | "gif" | "webp"
                            ) {
                                return Ok(HttpResponse::BadRequest().body("Unsupported image format"));
                            }

                            // Check if the image is a GIF by its subtype
                            let is_gif = mime_type.subtype().as_ref() == "gif";

                            // Generate a unique filename
                            let unique_id = Uuid::new_v4().to_string();
                            let extension = mime_type.subtype().as_str();
                            let sanitized_filename = format!("{}.{}", unique_id, extension);
                            let filepath = format!("{}{}", IMAGE_UPLOAD_DIR, sanitized_filename);
                            let filepath_clone = filepath.clone(); // Clone the filepath

                            // Save the image file asynchronously
                            let mut f = web::block(move || std::fs::File::create(&filepath)).await??;

                            while let Some(chunk) = field.next().await {
                                let data = chunk?;
                                f = web::block(move || f.write_all(&data).map(|_| f)).await??;
                            }

                            // Validate the image content using the cloned filepath
                            if let Err(_) = image::open(&filepath_clone) {
                                std::fs::remove_file(&filepath_clone)?;
                                return Ok(HttpResponse::BadRequest().body("Invalid image file"));
                            }

                            if is_gif {
                                // For GIFs, skip thumbnail generation
                                media_url = Some(format!("/uploads/images/{}", sanitized_filename));
                                media_type = Some(MediaType::Image);
                            } else {
                                // Generate a thumbnail for non-GIF images
                                let thumb_filename = format!("thumb_{}", sanitized_filename);
                                let thumb_path = format!("{}{}", IMAGE_THUMB_DIR, thumb_filename);
                                if let Ok(img) = image::open(&filepath_clone) {
                                    let thumb = image::imageops::thumbnail(&img, 200, 200);
                                    thumb.save(&thumb_path).ok();
                                    media_url = Some(format!("/thumbs/images/{}", thumb_filename));
                                    media_type = Some(MediaType::Image);
                                }

                                // If thumbnail creation failed, use the original image
                                if media_url.is_none() {
                                    media_url = Some(format!("/uploads/images/{}", sanitized_filename));
                                    media_type = Some(MediaType::Image);
                                }
                            }
                        }
                        mime::VIDEO => {
                            // Supported video subtypes
                            if mime_type.subtype().as_ref() != "mp4" {
                                return Ok(HttpResponse::BadRequest().body("Unsupported video format"));
                            }

                            // Generate a unique filename
                            let unique_id = Uuid::new_v4().to_string();
                            let extension = mime_type.subtype().as_str();
                            let sanitized_filename = format!("{}.{}", unique_id, extension);
                            let filepath = format!("{}{}", VIDEO_UPLOAD_DIR, sanitized_filename);

                            // Save the video file asynchronously
                            let mut f = web::block(move || std::fs::File::create(&filepath)).await??;

                            while let Some(chunk) = field.next().await {
                                let data = chunk?;
                                f = web::block(move || f.write_all(&data).map(|_| f)).await??;
                            }

                            // Basic validation: check if the file is a valid MP4
                            // Note: image::open won't validate videos. Consider using a video processing crate for robust validation.
                            // For simplicity, we'll skip validation here.

                            media_url = Some(format!("/uploads/videos/{}", sanitized_filename));
                            media_type = Some(MediaType::Video);
                        }
                        _ => {
                            return Ok(HttpResponse::BadRequest().body("Unsupported media type"));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Ensure that title and message are not empty
    if title.trim().is_empty() || message.trim().is_empty() {
        return Ok(HttpResponse::BadRequest()
            .content_type("text/html")
            .body(render_error_page("Bad Request", "Title and Message cannot be empty")));
    }

    let thread_id = count_threads(&db) + 1;
    let thread = Thread {
        id: thread_id,
        title: title.trim().to_string(),
        message: message.trim().to_string(),
        last_updated: Utc::now().timestamp(),
        media_url,
        media_type,
    };

    let key = format!("thread_{}", thread_id).into_bytes();
    let value = serde_json::to_vec(&thread).expect("Failed to serialize thread");

    if db.insert(key, value).is_ok() {
        Ok(HttpResponse::SeeOther()
            .append_header(("Location", "/"))
            .finish())
    } else {
        error!("Failed to insert thread into sled db");
        Ok(HttpResponse::InternalServerError()
            .content_type("text/html")
            .body(render_error_page("Internal Server Error", "Failed to create thread")))
    }
}

// Handler to create a new reply to an existing thread
async fn create_reply(
    db: web::Data<Arc<Db>>,
    form: web::Form<ReplyForm>,
) -> Result<HttpResponse, Error> {
    let parent_id = form.parent_id;
    let message = form.message.trim().to_string();

    // Ensure that message is not empty
    if message.is_empty() {
        return Ok(HttpResponse::BadRequest()
            .content_type("text/html")
            .body(render_error_page("Bad Request", "Message cannot be empty")));
    }

    let reply_id = count_replies(&db, parent_id) + 1;
    let reply = Reply {
        id: reply_id,
        message,
    };

    let key = format!("reply_{}_{}", parent_id, reply_id).into_bytes();
    let value = serde_json::to_vec(&reply).expect("Failed to serialize reply");

    if db.insert(key, value).is_ok() {
        // Update thread's last_updated timestamp
        let thread_key = format!("thread_{}", parent_id).into_bytes();
        if let Some(thread_bytes) = db.get(&thread_key).ok().flatten() {
            if let Ok(mut thread) = serde_json::from_slice::<Thread>(&thread_bytes) {
                thread.last_updated = Utc::now().timestamp();
                let updated = serde_json::to_vec(&thread).expect("Failed to serialize updated thread");
                db.insert(thread_key, updated).ok();
            }
        }

        Ok(HttpResponse::SeeOther()
            .append_header(("Location", format!("/thread/{}", parent_id)))
            .finish())
    } else {
        error!("Failed to insert reply into sled db");
        Ok(HttpResponse::InternalServerError()
            .content_type("text/html")
            .body(render_error_page("Internal Server Error", "Failed to post reply")))
    }
}

// Function to fetch all replies for a given thread from the Sled database
fn get_replies(db: &Db, parent_id: i32) -> Vec<Reply> {
    db.scan_prefix(format!("reply_{}", parent_id).as_bytes())
        .filter_map(|res| {
            if let Ok((_, value)) = res {
                serde_json::from_slice(&value).ok()
            } else {
                None
            }
        })
        .collect::<Vec<Reply>>()
}

// Function to count the total number of replies for a given thread
fn count_replies(db: &Db, parent_id: i32) -> i32 {
    db.scan_prefix(format!("reply_{}", parent_id).as_bytes()).count() as i32
}
