//! Non-matrix AI surfaces such as files and video.

mod files;
mod image;
mod video;

pub(crate) use self::files::{
    build_local_gemini_files_stream_attempt_source_for_kind,
    build_local_gemini_files_stream_plan_and_reports_for_kind,
    build_local_gemini_files_sync_attempt_source_for_kind,
    build_local_gemini_files_sync_plan_and_reports_for_kind,
    maybe_build_stream_local_gemini_files_decision_payload,
    maybe_build_sync_local_gemini_files_decision_payload,
};
pub(crate) use self::image::{
    apply_openai_image_tool_model, build_local_image_stream_attempt_source_for_kind,
    build_local_image_stream_plan_and_reports_for_kind,
    build_local_image_sync_attempt_source_for_kind,
    build_local_image_sync_plan_and_reports_for_kind, build_openai_image_passthrough_headers,
    build_openai_image_passthrough_json_body, build_openai_image_upstream_url_for_request,
    maybe_build_stream_local_image_decision_payload, maybe_build_sync_local_image_decision_payload,
    openai_image_uses_images_passthrough, set_local_openai_image_execution_exhausted_diagnostic,
};
pub(crate) use self::video::{
    build_local_video_sync_attempt_source_for_kind,
    build_local_video_sync_plan_and_reports_for_kind,
    maybe_build_sync_local_video_decision_payload,
};
