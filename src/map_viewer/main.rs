use common::{debug_renderer, map_file, map_polygonizer, system_window};
use sdl2::{event::Event, keyboard::Keycode};
use std::{path::PathBuf, time::Duration};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "map_compiler", about = "SquareWheel map compiler.")]
struct Opt
{
	/// Input file
	#[structopt(parse(from_os_str), short = "i")]
	input: Option<PathBuf>,

	#[structopt(long)]
	draw_raw_map: bool,

	#[structopt(long)]
	draw_polygonized_map: bool,

	#[structopt(long)]
	draw_polygon_normals: bool,
}

pub fn main()
{
	let opt = Opt::from_args();

	let mut window = system_window::SystemWindow::new();
	let mut camera_controller = common::camera_controller::CameraController::new();

	let mut map_file_parsed: Option<map_file::MapFileParsed> = None;
	let mut map_polygonized: Option<map_polygonizer::MapPolygonized> = None;
	if let Some(path) = &opt.input
	{
		let file_contents_str = std::fs::read_to_string(path).unwrap();
		map_file_parsed = map_file::parse_map_file_content(&file_contents_str).ok();
		if opt.draw_polygonized_map
		{
			if let Some(map_file) = &map_file_parsed
			{
				map_polygonized = Some(map_polygonizer::polygonize_map(map_file));
			}
		}
	}

	let frame_duration_s = 1.0 / 30.0;
	'running: loop
	{
		for event in window.get_events()
		{
			match event
			{
				Event::Quit { .. } |
				Event::KeyDown {
					keycode: Some(Keycode::Escape),
					..
				} => break 'running,
				_ =>
				{},
			}
		}

		camera_controller.update(&window.get_keyboard_state(), frame_duration_s);

		window.end_frame(|pixels, surface_info| {
			debug_renderer::draw_frame(
				pixels,
				surface_info,
				&debug_renderer::DrawOptions {
					draw_raw_map: opt.draw_raw_map,
					draw_polygonized_map: opt.draw_polygonized_map,
					draw_only_first_entity: false,
					draw_polygon_normals: opt.draw_polygon_normals,
				},
				&camera_controller.build_view_matrix(surface_info.width as f32, surface_info.height as f32),
				map_file_parsed.as_ref(),
				map_polygonized.as_ref(),
			)
		});

		std::thread::sleep(Duration::from_secs_f32(frame_duration_s));
	}
}
