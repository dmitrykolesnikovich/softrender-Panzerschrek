use super::{bsp_map_compact, clipping::*, clipping_polygon::*, math_types::*, matrix};

// List of visible BSP leafs tree for each leaf.
pub type LeafsVisibilityInfo = Vec<VisibleLeafsList>;
pub type VisibleLeafsList = Vec<u32>;
pub fn caclulate_pvs(map: &bsp_map_compact::BSPMap) -> LeafsVisibilityInfo
{
	let mut result = LeafsVisibilityInfo::with_capacity(map.leafs.len());
	for leaf_index in 0 .. map.leafs.len() as u32
	{
		result.push(calculate_pvs_for_leaf(map, leaf_index));

		let ratio_before = leaf_index * 256 / (map.leafs.len() as u32);
		let ratio_after = (leaf_index + 1) * 256 / (map.leafs.len() as u32);
		if ratio_after != ratio_before
		{
			print!(
				"\r{:03.2}% complete ({} of {} leafs)",
				((leaf_index + 1) as f32) * 100.0 / (map.leafs.len() as f32),
				leaf_index + 1,
				map.leafs.len()
			);
		}
	}
	println!("\nDone!");
	result
}

pub fn calculate_pvs_for_leaf(map: &bsp_map_compact::BSPMap, leaf_index: u32) -> VisibleLeafsList
{
	let mut visible_leafs_set = vec![false; map.leafs.len()];

	let leaf = &map.leafs[leaf_index as usize];
	for &portal_index in
		&map.leafs_portals[leaf.first_leaf_portal as usize .. (leaf.first_leaf_portal + leaf.num_leaf_portals) as usize]
	{
		let mut vis_leafs_data = vec![VisLeafData::default(); map.leafs.len()];
		calculate_pvs_for_leaf_portal(map, leaf_index, portal_index, &mut vis_leafs_data);

		for (index, vis_leaf_data) in vis_leafs_data.iter().enumerate()
		{
			if vis_leaf_data.bounds.is_some()
			{
				visible_leafs_set[index] = true;
			}
		}
	}

	visible_leafs_bit_set_to_leafs_list(&visible_leafs_set)
}

// TODO - use more advanced collection.
type VisibleLeafsBitSet = Vec<bool>;

fn visible_leafs_bit_set_to_leafs_list(visible_leafs_bit_set: &VisibleLeafsBitSet) -> VisibleLeafsList
{
	let mut result = VisibleLeafsList::new();
	for (i, &visible) in visible_leafs_bit_set.iter().enumerate()
	{
		if visible
		{
			result.push(i as u32);
		}
	}
	result
}

#[derive(Default, Copy, Clone)]
struct VisLeafData
{
	bounds: Option<ClippingPolygon>,
}

type VisLeafsData = Vec<VisLeafData>;

type SearchWaveElement = u32; // Leaf index.
type SearchWave = Vec<SearchWaveElement>;

fn calculate_pvs_for_leaf_portal(
	map: &bsp_map_compact::BSPMap,
	start_leaf_index: u32,
	start_portal_index: u32,
	vis_leafs_data: &mut VisLeafsData,
)
{
	let portal = &map.portals[start_portal_index as usize];
	let view_matrices = calculate_portal_view_matrices(map, start_leaf_index, portal);

	let mut cur_wave = SearchWave::new();
	let mut next_wave = SearchWave::new();

	cur_wave.push(start_leaf_index);

	let inf = 1e30;
	vis_leafs_data[start_leaf_index as usize].bounds = Some(ClippingPolygon::from_box(-inf, -inf, inf, inf));

	let max_itertions = 32;
	let mut num_iterations = 1;
	while !cur_wave.is_empty()
	{
		for &leaf_index in &cur_wave
		{
			let prev_leaf_bounds = vis_leafs_data[leaf_index as usize].bounds.unwrap();

			let leaf = &map.leafs[leaf_index as usize];
			for &portal_index in &map.leafs_portals
				[leaf.first_leaf_portal as usize .. (leaf.first_leaf_portal + leaf.num_leaf_portals) as usize]
			{
				let portal = &map.portals[portal_index as usize];

				let next_leaf_index = if portal.leafs[0] == leaf_index
				{
					portal.leafs[1]
				}
				else
				{
					portal.leafs[0]
				};

				let mut portal_bounds = if let Some(b) = project_portal(map, portal, &view_matrices)
				{
					b
				}
				else
				{
					continue;
				};

				portal_bounds.intersect(&prev_leaf_bounds);
				if portal_bounds.is_empty_or_invalid()
				{
					continue;
				}

				let vis_leaf_data = &mut vis_leafs_data[next_leaf_index as usize];
				if let Some(prev_bounds) = &mut vis_leaf_data.bounds
				{
					if prev_bounds.contains(&portal_bounds)
					{
						continue;
					}
					prev_bounds.extend(&portal_bounds);
				}
				else
				{
					vis_leaf_data.bounds = Some(portal_bounds);
				}
				next_wave.push(next_leaf_index);
			} // for portals.
		} // for wave elements.

		cur_wave.clear();
		std::mem::swap(&mut cur_wave, &mut next_wave);

		num_iterations += 1;
		if num_iterations >= max_itertions
		{
			break;
		}
	} // For wave steps.
}

fn calculate_portal_view_matrices(
	map: &bsp_map_compact::BSPMap,
	leaf_index: u32,
	portal: &bsp_map_compact::Portal,
) -> Vec<Mat4f>
{
	let mut plane = portal.plane;
	if portal.leafs[0] == leaf_index
	{
		plane = plane.get_inverted();
	}

	// Build set of projection matrices for each portal vertex. Camera points in direction of portal plane normal.

	let azimuth = plane.vec.y.atan2(plane.vec.x);
	let elevation = plane
		.vec
		.z
		.atan2((plane.vec.x * plane.vec.x + plane.vec.y * plane.vec.y).sqrt());

	let mut result = Vec::with_capacity(portal.num_vertices as usize);
	for vertex in &map.vertices[portal.first_vertex as usize .. (portal.first_vertex + portal.num_vertices) as usize]
	{
		// It's not important what exact FOV and viewport size to use.
		let viewport_size = 1.0;
		let mat = matrix::build_view_matrix(
			*vertex,
			Rad(azimuth),
			Rad(elevation),
			std::f32::consts::PI * 0.5,
			viewport_size,
			viewport_size,
		);
		result.push(mat.view_matrix);
	}

	result
}

fn project_portal(
	map: &bsp_map_compact::BSPMap,
	portal: &bsp_map_compact::Portal,
	view_matrices: &[Mat4f],
) -> Option<ClippingPolygon>
{
	const MAX_VERTICES: usize = 24;
	let mut vertices_transformed = [Vec3f::zero(); MAX_VERTICES]; // TODO - use uninitialized memory
	let mut vertices_transformed_z_clipped = [Vec3f::zero(); MAX_VERTICES]; // TODO - use uninitialized memory

	let mut result: Option<ClippingPolygon> = None;
	for view_matrix in view_matrices
	{
		let mut vertex_count = std::cmp::min(portal.num_vertices as usize, MAX_VERTICES);

		// Perform initial matrix tranformation, obtain 3d vertices in camera-aligned space.
		for (in_vertex, out_vertex) in map.vertices
			[(portal.first_vertex as usize) .. (portal.first_vertex as usize) + vertex_count]
			.iter()
			.zip(vertices_transformed.iter_mut())
		{
			let vertex_transformed = view_matrix * in_vertex.extend(1.0);
			*out_vertex = Vec3f::new(vertex_transformed.x, vertex_transformed.y, vertex_transformed.w);
		}

		// Perform z_near clipping. Use very small z_near to avoid clipping portals.
		const Z_NEAR: f32 = 1.0 / 4096.0;
		vertex_count = clip_3d_polygon_by_z_plane(
			&vertices_transformed[.. vertex_count],
			Z_NEAR,
			&mut vertices_transformed_z_clipped,
		);
		if vertex_count < 3
		{
			continue;
		}

		for vertex_transformed in &vertices_transformed_z_clipped[.. vertex_count]
		{
			let point = vertex_transformed.truncate() / vertex_transformed.z;
			if let Some(r) = &mut result
			{
				r.extend_with_point(&point);
			}
			else
			{
				result = Some(ClippingPolygon::from_point(&point));
			}
		}
	}

	result
}
