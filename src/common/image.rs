use super::color::*;
use sdl2::image::LoadSurface;

#[derive(Default, Clone)]
pub struct Image
{
	pub size: [u32; 2],
	pub pixels: Vec<Color32>,
}

pub fn load(file_path: &std::path::Path) -> Option<Image>
{
	let expected_format = sdl2::pixels::PixelFormatEnum::ARGB8888;
	if let Ok(mut surface) = sdl2::surface::Surface::from_file(file_path)
	{
		if surface.pixel_format_enum() != expected_format
		{
			if let Ok(surface_converted) = surface.convert_format(expected_format)
			{
				surface = surface_converted;
			}
			else
			{
				return None;
			}
		}

		let row_size = surface.pitch() / 4;
		let mut image = Image {
			size: [surface.width(), surface.height()],
			pixels: vec![Color32::from_rgb(0, 0, 0); (surface.height() * row_size) as usize],
		};

		surface.with_lock(|pixels| {
			// TODO - what if alignment is wrong?
			let pixels_32 = unsafe { pixels.align_to::<Color32>().1 };
			for y in 0 .. image.size[1]
			{
				let dst_start = y * image.size[0];
				let dst = &mut image.pixels[(dst_start as usize) .. (dst_start + image.size[0]) as usize];
				let src_start = y * row_size;
				let src = &pixels_32[(src_start as usize) .. (src_start + image.size[0]) as usize];

				dst.copy_from_slice(src);
			}
		});

		return Some(image);
	}

	None
}