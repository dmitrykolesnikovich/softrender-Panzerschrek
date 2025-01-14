#![cfg_attr(feature = "rasterizer_unchecked_div", feature(core_intrinsics))]

pub mod abstract_color;
pub mod commands_processor;
pub mod commands_queue;
pub mod config;
pub mod console;
pub mod debug_stats_printer;
pub mod depth_renderer;
pub mod draw_ordering;
pub mod dynamic_models_index;
pub mod equations;
pub mod fast_math;
pub mod frame_info;
pub mod frame_number;
pub mod game_interface;
pub mod host;
pub mod host_config;
pub mod inline_models_index;
pub mod light;
pub mod map_materials_processor;
pub mod map_visibility_calculator;
pub mod performance_counter;
pub mod postprocessor;
pub mod postprocessor_config;
pub mod rasterizer;
pub mod rect_splitting;
pub mod renderer;
pub mod renderer_config;
pub mod resources_manager;
pub mod resources_manager_config;
pub mod shadow_map;
pub mod surfaces;
pub mod text_printer;
pub mod textures;
pub mod ticks_counter;
pub mod triangle_model;
pub mod triangle_model_iqm;
pub mod triangle_model_loading;
pub mod triangle_model_md3;
pub mod triangle_models_rendering;
