use super::{bbox::*, clipping, lightmap, map_file_common, map_polygonizer, material, math_types::*, plane::*};
use std::{cell, rc};

pub use map_polygonizer::Polygon;

pub struct BSPTree
{
	pub root: BSPNodeChild,
	pub portals: Vec<LeafsPortalPtr>,
}

#[derive(Debug, Clone)]
pub enum BSPNodeChild
{
	NodeChild(BSPNodePtr),
	LeafChild(BSPLeafPtr),
}

#[derive(Debug)]
pub struct BSPNode
{
	pub plane: Plane,
	pub children: [BSPNodeChild; 2],
}

pub type BSPNodePtr = rc::Rc<cell::RefCell<BSPNode>>;

#[derive(Debug)]
pub struct BSPLeaf
{
	pub polygons: Vec<Polygon>,
	pub portals: Vec<LeafsPortalWeakPtr>,
}

pub type BSPLeafPtr = rc::Rc<cell::RefCell<BSPLeaf>>;

// Portal between two BSP leafs.
#[derive(Debug)]
pub struct LeafsPortal
{
	pub leaf_front: BSPLeafPtr,
	pub leaf_back: BSPLeafPtr,
	pub plane: Plane,
	pub vertices: Vec<Vec3f>,
}

pub type LeafsPortalPtr = rc::Rc<cell::RefCell<LeafsPortal>>;
pub type LeafsPortalWeakPtr = rc::Weak<cell::RefCell<LeafsPortal>>;

#[derive(Debug)]
pub struct SubmodelBSPNode
{
	// Dummy plane for leaf nodes.
	pub plane: Plane,
	// Contains polygons on splitter plane for nodes, contains  list of all polygons for leafs.
	pub polygons: Vec<Polygon>,
	pub children: [Option<Box<SubmodelBSPNode>>; 2],
}

pub fn build_leaf_bsp_tree(map_entities: &[map_polygonizer::Entity], materials: &material::MaterialsMap) -> BSPTree
{
	let world_entity = &map_entities[0];
	let bbox = build_bounding_box(&world_entity);

	// Build BSP tree for world entity.
	let mut tree_root = build_leaf_bsp_tree_r(filter_out_invisible_polygons(&world_entity.polygons, materials));

	// Build portals as links between BSP leafs.
	let mut portals = build_protals(&tree_root, &bbox, materials);
	set_leafs_portals(&portals);

	// Now we have graph of leafs and portals.
	// We need to remove ubnreachable leafs from this graph.
	// In order to do this mark each entity's leaf as reachable and mark as reachable all leafs, reachable through portals.
	let entities_positions = collect_entities_positions(&map_entities[1 ..]);
	let reachable_leafs = collect_reachable_leafs(&tree_root, &entities_positions);

	// Now correct BSP tree. Remove unreachable leafs and all nodes with only unreachable children.
	let mut leafs_removal_stats = UnreachelbeLeafsRemovalStats {
		total_leafs: 0,
		leafs_removed: 0,
	};
	let root_is_reachable = remove_unreachable_leafs_r(&mut tree_root, &reachable_leafs, &mut leafs_removal_stats);
	if !root_is_reachable
	{
		// TODO - what we should do in such case?
		println!("Warning, root node is unreachable!");
	}
	println!(
		"Bsp leafs initial: {}, unreachable leafs removed: {}",
		leafs_removal_stats.total_leafs, leafs_removal_stats.leafs_removed
	);

	// Remove portals between ureachable leafs from global portals list.
	remove_unreachable_portals(&mut portals, &reachable_leafs);

	// Normally reachable leafs should have no portals to unreachable leafs.
	// But anyway try to remove removed portals from list of portals for each leaf.
	remove_expired_portals_from_leafs_r(&mut tree_root);

	BSPTree {
		root: tree_root,
		portals,
	}
}

fn build_leaf_bsp_tree_r(mut in_polygons: Vec<Polygon>) -> BSPNodeChild
{
	let splitter_plane_opt = choose_best_splitter_plane(&in_polygons);
	if splitter_plane_opt.is_none()
	{
		// No splitter plane means this is a leaf.
		return BSPNodeChild::LeafChild(rc::Rc::new(cell::RefCell::new(BSPLeaf {
			polygons: in_polygons,
			portals: Vec::new(),
		})));
	}
	let splitter_plane = splitter_plane_opt.unwrap();

	let mut polygons_front = Vec::new();
	let mut polygons_back = Vec::new();
	for polygon in in_polygons.drain(..)
	{
		match get_polygon_position_relative_plane(&polygon, &splitter_plane)
		{
			PolygonPositionRelativePlane::Front | PolygonPositionRelativePlane::CoplanarFront =>
			{
				polygons_front.push(polygon);
			},
			PolygonPositionRelativePlane::Back | PolygonPositionRelativePlane::CoplanarBack =>
			{
				polygons_back.push(polygon);
			},
			PolygonPositionRelativePlane::Splitted =>
			{
				let (front_polygon, back_polygon) = split_polygon(&polygon, &splitter_plane);
				// Check for number of vertices is not needed here, but add anyway to avoid further problems if something is broken.
				if front_polygon.vertices.len() >= 3
				{
					polygons_front.push(front_polygon);
				}
				if back_polygon.vertices.len() >= 3
				{
					polygons_back.push(back_polygon);
				}
			},
		}
	}

	// HACK! Something went wrong and we processing leaf now.
	if polygons_front.is_empty()
	{
		return BSPNodeChild::LeafChild(rc::Rc::new(cell::RefCell::new(BSPLeaf {
			polygons: polygons_back,
			portals: Vec::new(),
		})));
	}
	if polygons_back.is_empty()
	{
		return BSPNodeChild::LeafChild(rc::Rc::new(cell::RefCell::new(BSPLeaf {
			polygons: polygons_front,
			portals: Vec::new(),
		})));
	}

	BSPNodeChild::NodeChild(rc::Rc::new(cell::RefCell::new(BSPNode {
		plane: splitter_plane,
		children: [
			build_leaf_bsp_tree_r(polygons_front),
			build_leaf_bsp_tree_r(polygons_back),
		],
	})))
}

// Returns None if can't find any situable splitter.
fn choose_best_splitter_plane(polygons: &[Polygon]) -> Option<Plane>
{
	let mut best_score_plane: Option<(f32, Plane)> = None;
	for polygon in polygons
	{
		if let Some(score) = get_splitter_plane_score(polygons, &polygon.plane)
		{
			if let Some((prev_score, _)) = best_score_plane
			{
				if score < prev_score
				{
					best_score_plane = Some((score, polygon.plane))
				}
			}
			else
			{
				best_score_plane = Some((score, polygon.plane))
			}
		}
	}

	best_score_plane.map(|x| x.1)
}

// smaller score means better
// None score means plane is not a splitter
fn get_splitter_plane_score(polygons: &[Polygon], plane: &Plane) -> Option<f32>
{
	let mut polygons_front = 0i32;
	let mut polygons_back = 0i32;
	let mut polygons_coplanar_front = 0i32;
	let mut polygons_coplanar_back = 0i32;
	let mut polygons_splitted = 0i32;
	for polygon in polygons
	{
		match get_polygon_position_relative_plane(polygon, plane)
		{
			PolygonPositionRelativePlane::Front =>
			{
				polygons_front += 1;
			},
			PolygonPositionRelativePlane::Back =>
			{
				polygons_back += 1;
			},
			PolygonPositionRelativePlane::CoplanarFront =>
			{
				polygons_coplanar_front += 1;
			},
			PolygonPositionRelativePlane::CoplanarBack =>
			{
				polygons_coplanar_back += 1;
			},
			PolygonPositionRelativePlane::Splitted =>
			{
				polygons_splitted += 1;
			},
		}
	}

	let polygons_front_total = polygons_front + polygons_coplanar_front;
	let polygons_back_total = polygons_back + polygons_coplanar_back;

	// All polygons are at one of sides. So, this is not a splitter.
	if polygons_splitted == 0 && (polygons_front_total == 0 || polygons_back_total == 0)
	{
		return None;
	}

	// TODO - tune this carefully.
	const SPLITTED_POLYGON_SCALE: i32 = 5;
	let base_score = (polygons_front_total - polygons_back_total).abs() + SPLITTED_POLYGON_SCALE * polygons_splitted;

	// Make score greater (worse) for planes non-parallel to axis planes.
	let mut num_zero_normal_components = 0;
	let plane_vec_as_array: &[f32; 3] = plane.vec.as_ref();
	for component in plane_vec_as_array
	{
		if *component == 0.0
		{
			num_zero_normal_components += 1;
		}
	}

	let mut score_scaled = base_score as f32;
	if num_zero_normal_components == 0
	{
		score_scaled *= 2.0;
	}
	if num_zero_normal_components == 1
	{
		score_scaled *= 1.5;
	}

	Some(score_scaled)
}

pub fn build_submodel_bsp_tree(
	submodel: &map_polygonizer::Entity,
	materials: &material::MaterialsMap,
) -> SubmodelBSPNode
{
	let polygons_filtered = filter_out_invisible_polygons(&submodel.polygons, materials);
	if polygons_filtered.is_empty()
	{
		return SubmodelBSPNode {
			polygons: Vec::new(),
			plane: Plane {
				vec: Vec3f::zero(),
				dist: 0.0,
			},
			children: [None, None],
		};
	}

	build_submodel_bsp_tree_r(polygons_filtered)
}

fn build_submodel_bsp_tree_r(mut in_polygons: Vec<Polygon>) -> SubmodelBSPNode
{
	if let Some(splitter_plane) = choose_submodel_best_splitter_plane(&in_polygons)
	{
		let mut polygons_front = Vec::new();
		let mut polygons_back = Vec::new();
		let mut polygons_coplanar = Vec::new();
		for polygon in in_polygons.drain(..)
		{
			match get_polygon_position_relative_plane(&polygon, &splitter_plane)
			{
				PolygonPositionRelativePlane::Front =>
				{
					polygons_front.push(polygon);
				},
				PolygonPositionRelativePlane::Back =>
				{
					polygons_back.push(polygon);
				},
				PolygonPositionRelativePlane::CoplanarFront | PolygonPositionRelativePlane::CoplanarBack =>
				{
					polygons_coplanar.push(polygon);
				},
				PolygonPositionRelativePlane::Splitted =>
				{
					let (front_polygon, back_polygon) = split_polygon(&polygon, &splitter_plane);
					// Check for number of vertices is not needed here, but add anyway to avoid further problems if something is broken.
					if front_polygon.vertices.len() >= 3
					{
						polygons_front.push(front_polygon);
					}
					if back_polygon.vertices.len() >= 3
					{
						polygons_back.push(back_polygon);
					}
				},
			}
		}

		let child_front = if polygons_front.is_empty()
		{
			None
		}
		else
		{
			Some(Box::new(build_submodel_bsp_tree_r(polygons_front)))
		};

		let child_back = if polygons_back.is_empty()
		{
			None
		}
		else
		{
			Some(Box::new(build_submodel_bsp_tree_r(polygons_back)))
		};

		SubmodelBSPNode {
			polygons: polygons_coplanar,
			plane: splitter_plane,
			children: [child_front, child_back],
		}
	}
	else
	{
		let dummy_plane = in_polygons[0].plane;
		SubmodelBSPNode {
			polygons: in_polygons,
			plane: dummy_plane,
			children: [None, None],
		}
	}
}

// Returns None if can't find any situable splitter.
fn choose_submodel_best_splitter_plane(polygons: &[Polygon]) -> Option<Plane>
{
	let mut best_score_plane: Option<(f32, Plane)> = None;
	for polygon in polygons
	{
		if let Some(score) = get_submodel_splitter_plane_score(polygons, &polygon.plane)
		{
			if let Some((prev_score, _)) = best_score_plane
			{
				if score < prev_score
				{
					best_score_plane = Some((score, polygon.plane))
				}
			}
			else
			{
				best_score_plane = Some((score, polygon.plane))
			}
		}
	}

	best_score_plane.map(|x| x.1)
}

// smaller score means better
// None score means plane is not a splitter
fn get_submodel_splitter_plane_score(polygons: &[Polygon], plane: &Plane) -> Option<f32>
{
	let mut polygons_front = 0i32;
	let mut polygons_back = 0i32;
	let mut polygons_splitted = 0i32;
	let mut polygons_complanar_front = 0i32;
	let mut polygons_complanar_back = 0i32;
	for polygon in polygons
	{
		match get_polygon_position_relative_plane(polygon, plane)
		{
			PolygonPositionRelativePlane::Front =>
			{
				polygons_front += 1;
			},
			PolygonPositionRelativePlane::Back =>
			{
				polygons_back += 1;
			},
			PolygonPositionRelativePlane::CoplanarFront =>
			{
				polygons_complanar_front += 1;
			},
			PolygonPositionRelativePlane::CoplanarBack =>
			{
				polygons_complanar_back += 1;
			},
			PolygonPositionRelativePlane::Splitted =>
			{
				polygons_splitted += 1;
			},
		}
	}

	let polygons_complanar_total = polygons_complanar_front + polygons_complanar_back;

	if polygons_complanar_total == polygons.len() as i32
	{
		// All polygons are in single plane. We can just create leaf node.
		return None;
	}
	if polygons_splitted == 0 &&
		(polygons_complanar_front == 0 || polygons_complanar_back == 0) &&
		(polygons_front == 0 || polygons_back == 0)
	{
		// In case if all polygons of this plane facing towards single direction and all other polygons are located on one side
		// there is no reason to create splitter using this plane.
		// Anyway polygons on this plane will be ordered with all other polygons.
		return None;
	}

	// TODO - tune this carefully.
	const SPLITTED_POLYGON_SCALE: i32 = 5;
	let base_score = (polygons_front - polygons_back).abs() + SPLITTED_POLYGON_SCALE * polygons_splitted;

	// Make score greater (worse) for planes non-parallel to axis planes.
	let mut num_zero_normal_components = 0;
	let plane_vec_as_array: &[f32; 3] = plane.vec.as_ref();
	for component in plane_vec_as_array
	{
		if *component == 0.0
		{
			num_zero_normal_components += 1;
		}
	}

	let mut score_scaled = base_score as f32;
	if num_zero_normal_components == 0
	{
		score_scaled *= 2.0;
	}
	if num_zero_normal_components == 1
	{
		score_scaled *= 1.5;
	}

	Some(score_scaled)
}

// Returns pair of front and back polygons.
fn split_polygon(in_polygon: &Polygon, plane: &Plane) -> (Polygon, Polygon)
{
	let mut polygon_front = Polygon {
		plane: in_polygon.plane,
		texture_info: in_polygon.texture_info.clone(),
		vertices: Vec::new(),
	};
	let mut polygon_back = Polygon {
		plane: in_polygon.plane,
		texture_info: in_polygon.texture_info.clone(),
		vertices: Vec::new(),
	};

	let mut prev_vert = in_polygon.vertices.last().unwrap();
	let mut prev_vert_pos = get_point_position_relative_plane(&prev_vert, plane);
	for vert in &in_polygon.vertices
	{
		let vert_pos = get_point_position_relative_plane(&vert, plane);

		match vert_pos
		{
			PointPositionRelativePlane::Front =>
			{
				if prev_vert_pos == PointPositionRelativePlane::Back
				{
					let intersection = clipping::get_line_plane_intersection(prev_vert, vert, plane);
					polygon_back.vertices.push(intersection);
					polygon_front.vertices.push(intersection);
				}
				polygon_front.vertices.push(*vert);
			},
			PointPositionRelativePlane::Back =>
			{
				if prev_vert_pos == PointPositionRelativePlane::Front
				{
					let intersection = clipping::get_line_plane_intersection(prev_vert, vert, plane);
					polygon_front.vertices.push(intersection);
					polygon_back.vertices.push(intersection);
				}
				polygon_back.vertices.push(*vert);
			},
			PointPositionRelativePlane::OnPlane =>
			{
				polygon_front.vertices.push(*vert);
				polygon_back.vertices.push(*vert);
			},
		};

		prev_vert = vert;
		prev_vert_pos = vert_pos;
	}

	(polygon_front, polygon_back)
}

fn filter_out_invisible_polygons(polygons: &[Polygon], materials: &material::MaterialsMap) -> Vec<Polygon>
{
	let mut result = Vec::new();

	for polygon in polygons
	{
		if let Some(material) = materials.get(&polygon.texture_info.texture)
		{
			if !material.bsp
			{
				continue;
			}
		}

		result.push(polygon.clone());
	}
	result
}

fn build_bounding_box(entity: &map_polygonizer::Entity) -> BBox
{
	let inf = 1.0e8;
	let bbox_extend = 128.0;
	let mut bbox = BBox {
		min: Vec3f::new(inf, inf, inf),
		max: Vec3f::new(-inf, -inf, -inf),
	};
	for polygon in &entity.polygons
	{
		for v in &polygon.vertices
		{
			bbox.extend_with_point(v);
		}
	}
	bbox.min -= Vec3f::new(bbox_extend, bbox_extend, bbox_extend);
	bbox.max += Vec3f::new(bbox_extend, bbox_extend, bbox_extend);

	bbox
}

#[derive(PartialEq, Eq)]
enum PolygonPositionRelativePlane
{
	Front,
	Back,
	CoplanarFront,
	CoplanarBack,
	Splitted,
}

fn get_polygon_position_relative_plane(polygon: &Polygon, plane: &Plane) -> PolygonPositionRelativePlane
{
	let mut vertices_front = 0;
	let mut vertices_back = 0;
	for v in &polygon.vertices
	{
		match get_point_position_relative_plane(v, plane)
		{
			PointPositionRelativePlane::Front =>
			{
				vertices_front += 1;
			},
			PointPositionRelativePlane::Back =>
			{
				vertices_back += 1;
			},
			PointPositionRelativePlane::OnPlane =>
			{},
		};
	}

	if vertices_front != 0 && vertices_back != 0
	{
		PolygonPositionRelativePlane::Splitted
	}
	else if vertices_front != 0
	{
		PolygonPositionRelativePlane::Front
	}
	else if vertices_back != 0
	{
		PolygonPositionRelativePlane::Back
	}
	else if polygon.plane.vec.dot(plane.vec) >= 0.0
	{
		PolygonPositionRelativePlane::CoplanarFront
	}
	else
	{
		PolygonPositionRelativePlane::CoplanarBack
	}
}

#[derive(PartialEq, Eq)]
enum PointPositionRelativePlane
{
	Front,
	Back,
	OnPlane,
}

const POINT_POSITION_EPS: f32 = 1.0 / 16.0;

fn get_point_position_relative_plane(point: &Vec3f, plane: &Plane) -> PointPositionRelativePlane
{
	// Polygon vector is unnormalized. So, scale epsilon to length of this vector.
	let dist_scaled = point.dot(plane.vec) - plane.dist;
	let eps_scaled = POINT_POSITION_EPS * plane.vec.magnitude();
	if dist_scaled > eps_scaled
	{
		PointPositionRelativePlane::Front
	}
	else if dist_scaled < -eps_scaled
	{
		PointPositionRelativePlane::Back
	}
	else
	{
		PointPositionRelativePlane::OnPlane
	}
}

fn build_protals(node: &BSPNodeChild, map_bbox: &BBox, materials: &material::MaterialsMap) -> Vec<LeafsPortalPtr>
{
	let mut splitter_nodes = Vec::new();
	let mut leaf_portals_by_node = LeafPortalsInitialByNode::new();
	build_protals_r(
		node,
		&mut splitter_nodes,
		&mut leaf_portals_by_node,
		map_bbox,
		materials,
	);

	let mut result = Vec::new();
	for (_node, portals) in leaf_portals_by_node
	{
		for result_portal in build_leafs_portals(&portals, materials)
		{
			result.push(rc::Rc::new(cell::RefCell::new(result_portal)));
		}
	}
	result
}

struct NodeForPortalsBuild
{
	node: BSPNodePtr,
	is_front: bool,
}

struct LeafPortalInitial
{
	vertices: Vec<Vec3f>,
	plane: Plane,
	leaf: BSPLeafPtr,
	is_front: bool,
}

type LeafPortalsInitialByNode = std::collections::HashMap<*const BSPNode, Vec<LeafPortalInitial>>;

fn build_protals_r(
	node_child: &BSPNodeChild,
	splitter_nodes: &mut Vec<NodeForPortalsBuild>,
	leaf_portals_by_node: &mut LeafPortalsInitialByNode,
	map_bbox: &BBox,
	materials: &material::MaterialsMap,
)
{
	match node_child
	{
		BSPNodeChild::NodeChild(node) =>
		{
			splitter_nodes.push(NodeForPortalsBuild {
				node: node.clone(),
				is_front: true,
			});
			build_protals_r(
				&node.borrow().children[0],
				splitter_nodes,
				leaf_portals_by_node,
				map_bbox,
				materials,
			);
			splitter_nodes.pop();

			splitter_nodes.push(NodeForPortalsBuild {
				node: node.clone(),
				is_front: false,
			});
			build_protals_r(
				&node.borrow().children[1],
				splitter_nodes,
				leaf_portals_by_node,
				map_bbox,
				materials,
			);
			splitter_nodes.pop();
		},
		BSPNodeChild::LeafChild(leaf_ptr) =>
		{
			build_leaf_portals(leaf_ptr, &splitter_nodes, map_bbox, materials, leaf_portals_by_node);
		},
	}
}

fn build_leaf_portals(
	leaf_ptr: &BSPLeafPtr,
	splitter_nodes: &[NodeForPortalsBuild],
	map_bbox: &BBox,
	materials: &material::MaterialsMap,
	leaf_portals_by_node: &mut LeafPortalsInitialByNode,
)
{
	let leaf = &leaf_ptr.borrow();
	// For each splitter plane create portal polygon - bounded with all other splitter planes and leaf polygons.

	let mut cut_planes = Vec::<Plane>::new();
	for splitter_node in splitter_nodes
	{
		let node = splitter_node.node.borrow();
		if splitter_node.is_front
		{
			cut_planes.push(node.plane.get_inverted());
		}
		else
		{
			cut_planes.push(node.plane);
		}
	}
	for polygon in &leaf.polygons
	{
		if let Some(material) = materials.get(&polygon.texture_info.texture)
		{
			if !material.blocks_view
			{
				continue;
			}
		}
		cut_planes.push(Plane {
			vec: -polygon.plane.vec,
			dist: -polygon.plane.dist,
		});
	}

	cut_planes.push(Plane {
		vec: Vec3f::new(1.0, 0.0, 0.0),
		dist: map_bbox.max.x,
	});
	cut_planes.push(Plane {
		vec: Vec3f::new(-1.0, 0.0, 0.0),
		dist: -map_bbox.min.x,
	});
	cut_planes.push(Plane {
		vec: Vec3f::new(0.0, 1.0, 0.0),
		dist: map_bbox.max.y,
	});
	cut_planes.push(Plane {
		vec: Vec3f::new(0.0, -1.0, 0.0),
		dist: -map_bbox.min.y,
	});
	cut_planes.push(Plane {
		vec: Vec3f::new(0.0, 0.0, 1.0),
		dist: map_bbox.max.z,
	});
	cut_planes.push(Plane {
		vec: Vec3f::new(0.0, 0.0, -1.0),
		dist: -map_bbox.min.z,
	});

	for splitter_node in splitter_nodes
	{
		let node = splitter_node.node.borrow();
		let portal_plane = if splitter_node.is_front
		{
			node.plane.get_inverted()
		}
		else
		{
			node.plane
		};

		let mut portal_vertices = Vec::new();
		for i in 0 .. cut_planes.len()
		{
			let cut_plane_i = cut_planes[i];
			if cut_plane_i == portal_plane
			{
				continue;
			}
			if are_planes_almost_parallel(&portal_plane, &cut_plane_i)
			{
				continue;
			}

			for j in i + 1 .. cut_planes.len()
			{
				let cut_plane_j = cut_planes[j];
				if cut_plane_j == portal_plane
				{
					continue;
				}
				if cut_plane_j == cut_plane_i
				{
					continue;
				}
				if are_planes_almost_parallel(&portal_plane, &cut_plane_j)
				{
					continue;
				}
				if are_planes_almost_parallel(&cut_plane_i, &cut_plane_j)
				{
					continue;
				}

				let mat = Mat3f::from_cols(portal_plane.vec, cut_plane_i.vec, cut_plane_j.vec).transpose();
				let inv_mat_opt = mat.invert();
				if inv_mat_opt.is_none()
				{
					continue; // No solution - some planes are parallel.
				}
				let intersection_point =
					inv_mat_opt.unwrap() * Vec3f::new(portal_plane.dist, cut_plane_i.dist, cut_plane_j.dist);

				let mut is_behind_another_plane = false;
				for k in 0 .. cut_planes.len()
				{
					if k == i || k == j
					{
						continue;
					}
					let plane_k = cut_planes[k];
					if plane_k == portal_plane
					{
						continue;
					}
					if intersection_point.dot(plane_k.vec) > plane_k.dist + plane_k.vec.magnitude() * POINT_POSITION_EPS
					{
						is_behind_another_plane = true;
						break;
					}
				} // for k

				if !is_behind_another_plane
				{
					portal_vertices.push(intersection_point);
				}
			} // for j
		} // for i

		if portal_vertices.len() < 3
		{
			continue;
		}

		let portal_vertices_deduplicated = map_polygonizer::remove_duplicate_vertices(&portal_vertices);
		if portal_vertices_deduplicated.len() < 3
		{
			continue;
		}

		let portal_vertices_sorted =
			map_polygonizer::sort_convex_polygon_vertices(portal_vertices_deduplicated, &node.plane);
		if portal_vertices_sorted.len() < 3
		{
			continue;
		}

		let portal = LeafPortalInitial {
			vertices: portal_vertices_sorted,
			leaf: leaf_ptr.clone(),
			plane: node.plane,
			is_front: splitter_node.is_front,
		};

		let ptr = (&*splitter_node.node.borrow()) as *const BSPNode;
		if !leaf_portals_by_node.contains_key(&ptr)
		{
			leaf_portals_by_node.insert(ptr, Vec::new());
		}
		leaf_portals_by_node.get_mut(&ptr).unwrap().push(portal);
	} // for portal planes
}

// Iterate over all pairs of portals of same node.
// Search for intersection of such portals.
fn build_leafs_portals(in_portals: &[LeafPortalInitial], materials: &material::MaterialsMap) -> Vec<LeafsPortal>
{
	let mut result = Vec::new();
	for portal_front in in_portals
	{
		if !portal_front.is_front
		{
			continue;
		}

		let plane = portal_front.plane;

		for portal_back in in_portals
		{
			if portal_back.is_front
			{
				continue;
			}

			let portals_intersection =
				build_portals_intersection(&plane, &portal_back.vertices, &portal_front.vertices);
			if portals_intersection.len() < 3
			{
				continue;
			}

			if portal_is_fully_covered_by_leaf_polygons(
				&plane,
				&portals_intersection,
				&portal_front.leaf.borrow(),
				materials,
			) || portal_is_fully_covered_by_leaf_polygons(
				&plane,
				&portals_intersection,
				&portal_back.leaf.borrow(),
				materials,
			)
			{
				continue;
			}

			result.push(LeafsPortal {
				leaf_front: portal_front.leaf.clone(),
				leaf_back: portal_back.leaf.clone(),
				plane,
				vertices: portals_intersection,
			});
		}
	}

	result
}

// Return < 3 vertices if failed.
fn build_portals_intersection(plane: &Plane, vertices0: &[Vec3f], vertices1: &[Vec3f]) -> Vec<Vec3f>
{
	// TODO - avoid calculations based on polygon edge equations.
	// SLightly non-convex polygon may ruin portals intersection calculation.
	let mut clip_planes = Vec::new();

	let mut prev_v = vertices0.last().unwrap();
	for v in vertices0
	{
		let vec = (prev_v - v).cross(plane.vec);
		clip_planes.push(Plane { vec, dist: vec.dot(*v) });
		prev_v = v;
	}
	let mut prev_v = vertices1.last().unwrap();
	for v in vertices1
	{
		let vec = (prev_v - v).cross(plane.vec);
		clip_planes.push(Plane { vec, dist: vec.dot(*v) });
		prev_v = v;
	}

	// Build set of vertices based on input planes.
	let mut vertices = Vec::new();
	for i in 0 .. clip_planes.len()
	{
		let plane_i = clip_planes[i];
		for j in i + 1 .. clip_planes.len()
		{
			let plane_j = clip_planes[j];
			if plane_j == plane_i
			{
				continue;
			}
			if are_planes_almost_parallel(&plane_i, &plane_j)
			{
				continue;
			}

			// Find intersection point between portal side planes and plane of portal.
			let mat = Mat3f::from_cols(plane.vec, plane_i.vec, plane_j.vec).transpose();
			let inv_mat_opt = mat.invert();
			if inv_mat_opt.is_none()
			{
				continue; // No solution - some planes are parallel.
			}
			let intersection_point = inv_mat_opt.unwrap() * Vec3f::new(plane.dist, plane_i.dist, plane_j.dist);

			let mut is_behind_another_plane = false;
			for k in 0 .. clip_planes.len()
			{
				if k == i || k == j
				{
					continue;
				}
				let plane_k = clip_planes[k];
				if plane_k == plane_i || plane_k == plane_j
				{
					continue;
				}
				if intersection_point.dot(plane_k.vec) > plane_k.dist + plane_k.vec.magnitude() * POINT_POSITION_EPS
				{
					is_behind_another_plane = true;
					break;
				}
			} // for k

			if !is_behind_another_plane
			{
				vertices.push(intersection_point);
			}
		} // for j
	} // for i

	if vertices.len() < 3
	{
		return vertices;
	}

	let vertices_deduplicated = map_polygonizer::remove_duplicate_vertices(&vertices);
	if vertices_deduplicated.len() < 3
	{
		return vertices_deduplicated;
	}

	map_polygonizer::sort_convex_polygon_vertices(vertices_deduplicated, &plane)
}

fn portal_is_fully_covered_by_leaf_polygons(
	portal_plane: &Plane,
	portal_vertices: &[Vec3f],
	leaf: &BSPLeaf,
	materials: &material::MaterialsMap,
) -> bool
{
	// Perform basic portals filtering.
	// Remove portals that are fully covered by one of leaf polygons.
	// Generally we should check for coverage by multiple polygons, but not now.

	const PORTAL_POLYGON_COVERAGE_EPS: f32 = 0.25;

	for polygon in &leaf.polygons
	{
		if let Some(material) = materials.get(&polygon.texture_info.texture)
		{
			if !material.blocks_view
			{
				continue;
			}
		}

		let polygon_position = get_polygon_position_relative_plane(polygon, portal_plane);
		if !(polygon_position == PolygonPositionRelativePlane::CoplanarFront ||
			polygon_position == PolygonPositionRelativePlane::CoplanarBack)
		{
			continue;
		}

		let mut prev_polygon_vertex = polygon.vertices.last().unwrap();
		let mut portal_is_inside_polygon = true;
		for polygon_vertex in &polygon.vertices
		{
			let vec = (prev_polygon_vertex - polygon_vertex).cross(polygon.plane.vec);
			let eps_scaled = PORTAL_POLYGON_COVERAGE_EPS * vec.magnitude();
			let cut_plane = Plane {
				vec,
				dist: vec.dot(*polygon_vertex),
			};

			let mut all_vertices_are_inside = true;
			for portal_vertex in portal_vertices
			{
				if portal_vertex.dot(cut_plane.vec) > cut_plane.dist + eps_scaled
				{
					all_vertices_are_inside = false;
					break;
				}
			}

			prev_polygon_vertex = polygon_vertex;

			if !all_vertices_are_inside
			{
				portal_is_inside_polygon = false;
				break;
			}
		} // for polygon edges

		if portal_is_inside_polygon
		{
			return true;
		}
	} // for polygons

	false
}

fn are_planes_almost_parallel(plane0: &Plane, plane1: &Plane) -> bool
{
	(plane0.vec.cross(plane1.vec).magnitude() / plane0.vec.dot(plane1.vec)).abs() < 0.0001
}

fn set_leafs_portals(portals: &[LeafsPortalPtr])
{
	for portal_ptr in portals
	{
		let portal_ptr_weak = rc::Rc::downgrade(portal_ptr);
		let portal = portal_ptr.borrow();
		portal.leaf_front.borrow_mut().portals.push(portal_ptr_weak.clone());
		portal.leaf_back.borrow_mut().portals.push(portal_ptr_weak);
	}
}

fn collect_entities_positions(map_entities: &[map_polygonizer::Entity]) -> Vec<Vec3f>
{
	let mut result = Vec::new();
	for entity in map_entities
	{
		if let Some(origin_str) = entity.keys.get("origin")
		{
			if let Ok(origin) = map_file_common::parse_vec3(origin_str)
			{
				result.push(origin);
			}
		}
	}
	result
}

type ReachableLeafsMap = std::collections::HashMap<*const BSPLeaf, BSPLeafPtr>;

fn collect_reachable_leafs(tree_root: &BSPNodeChild, start_points: &[Vec3f]) -> ReachableLeafsMap
{
	let mut reachable_leafs = ReachableLeafsMap::new();
	for point in start_points
	{
		let leaf = get_leaf_for_point(tree_root, point);
		collect_reachable_leafs_r(&leaf, &mut reachable_leafs);
	}
	reachable_leafs
}

fn get_leaf_for_point(node_child: &BSPNodeChild, point: &Vec3f) -> BSPLeafPtr
{
	match node_child
	{
		BSPNodeChild::NodeChild(node_ptr) =>
		{
			let node = node_ptr.borrow();
			if node.plane.vec.dot(*point) >= node.plane.dist
			{
				get_leaf_for_point(&node.children[0], point)
			}
			else
			{
				get_leaf_for_point(&node.children[1], point)
			}
		},
		BSPNodeChild::LeafChild(leaf_ptr) => leaf_ptr.clone(),
	}
}

fn collect_reachable_leafs_r(leaf_ptr: &BSPLeafPtr, reachable_leafs: &mut ReachableLeafsMap)
{
	let leaf = leaf_ptr.borrow();
	let leaf_raw_ptr = (&*leaf) as *const BSPLeaf;
	if reachable_leafs.contains_key(&leaf_raw_ptr)
	{
		return;
	}

	reachable_leafs.insert(leaf_raw_ptr, leaf_ptr.clone());

	for portal_ptr_weak in &leaf.portals
	{
		let protal_ptr = portal_ptr_weak.upgrade().unwrap();
		let portal = protal_ptr.borrow();
		collect_reachable_leafs_r(&portal.leaf_front, reachable_leafs);
		collect_reachable_leafs_r(&portal.leaf_back, reachable_leafs);
	}
}

struct UnreachelbeLeafsRemovalStats
{
	total_leafs: u32,
	leafs_removed: u32,
}

// Returns "true" if need to preserve this child.
fn remove_unreachable_leafs_r(
	node_child: &mut BSPNodeChild,
	reachable_leafs: &ReachableLeafsMap,
	stats: &mut UnreachelbeLeafsRemovalStats,
) -> bool
{
	match node_child
	{
		BSPNodeChild::NodeChild(node_ptr) =>
		{
			let mut node = node_ptr.borrow_mut();
			let preserve_front = remove_unreachable_leafs_r(&mut node.children[0], reachable_leafs, stats);
			let preserve_back = remove_unreachable_leafs_r(&mut node.children[1], reachable_leafs, stats);
			if !preserve_front && !preserve_back
			{
				return false;
			}
			if !preserve_back
			{
				let child = node.children[0].clone();
				drop(node);
				*node_child = child;
			}
			else if !preserve_front
			{
				let child = node.children[1].clone();
				drop(node);
				*node_child = child;
			}

			true
		},
		BSPNodeChild::LeafChild(leaf_ptr) =>
		{
			stats.total_leafs += 1;

			let leaf = leaf_ptr.borrow();
			let leaf_raw_ptr = (&*leaf) as *const BSPLeaf;
			let is_reachable = reachable_leafs.contains_key(&leaf_raw_ptr);

			if !is_reachable
			{
				stats.leafs_removed += 1;
			}

			is_reachable
		},
	}
}

fn remove_unreachable_portals(portals: &mut Vec<LeafsPortalPtr>, reachable_leafs: &ReachableLeafsMap)
{
	portals.retain(|portal_ptr| {
		let portal = portal_ptr.borrow();
		for leaf_ptr in [&portal.leaf_front, &portal.leaf_back]
		{
			let leaf = leaf_ptr.borrow();
			let leaf_raw_ptr = (&*leaf) as *const BSPLeaf;
			if !reachable_leafs.contains_key(&leaf_raw_ptr)
			{
				return false;
			}
		}
		true
	});
}

fn remove_expired_portals_from_leafs_r(node_child: &mut BSPNodeChild)
{
	match node_child
	{
		BSPNodeChild::NodeChild(node_ptr) =>
		{
			let mut node = node_ptr.borrow_mut();
			for child in &mut node.children
			{
				remove_expired_portals_from_leafs_r(child);
			}
		},
		BSPNodeChild::LeafChild(leaf_ptr) =>
		{
			leaf_ptr
				.borrow_mut()
				.portals
				.retain(|portal_weak_ptr| portal_weak_ptr.strong_count() > 0);
		},
	}
}

pub fn split_long_polygons(polygons: &[Polygon], materials: &material::MaterialsMap) -> Vec<Polygon>
{
	let mut result = Vec::new();
	for polygon in polygons
	{
		let mut need_split = true;
		if let Some(material) = materials.get(&polygon.texture_info.texture)
		{
			if !material.light && material.skybox.is_some()
			{
				need_split = false;
			}
		}

		if need_split
		{
			split_long_polygon_r(polygon, &mut result, 0)
		}
		else
		{
			result.push(polygon.clone());
		}
	}
	result
}

fn split_long_polygon_r(polygon: &Polygon, out_polygons: &mut Vec<Polygon>, recursion_depth: usize)
{
	if polygon.vertices.len() < 3
	{
		// Something is broken.
		return;
	}
	if recursion_depth > 10
	{
		// Something is really broken.
		return;
	}

	let inf = (1 << 29) as f32;
	for i in 0 .. 2
	{
		let mut tc_min = inf;
		let mut tc_max = -inf;
		for &v in &polygon.vertices
		{
			let tc =
				polygon.texture_info.tex_coord_equation[i].vec.dot(v) + polygon.texture_info.tex_coord_equation[i].dist;
			if tc < tc_min
			{
				tc_min = tc;
			}
			if tc > tc_max
			{
				tc_max = tc;
			}
		}

		let tc_min_int = tc_min.floor() as i32;
		let tc_max_int = tc_max.ceil() as i32;
		let lightmap_size = lightmap::get_lightmap_size(tc_min_int, tc_max_int);
		if lightmap_size <= lightmap::MAX_LIGHTMAP_SIZE
		{
			continue;
		}

		// Split this polygon recursively.

		// Round split plane position to lightmap grid.
		let middle_tc = (tc_max_int + tc_min_int) >> 1;
		let split_plane_shift = (middle_tc & !((lightmap::LIGHTMAP_SCALE - 1) as i32)) as f32;

		let split_plane = Plane {
			vec: polygon.texture_info.tex_coord_equation[i].vec,
			dist: -polygon.texture_info.tex_coord_equation[i].dist + split_plane_shift,
		};

		let (p0, p1) = split_polygon(polygon, &split_plane);
		split_long_polygon_r(&p0, out_polygons, recursion_depth + 1);
		split_long_polygon_r(&p1, out_polygons, recursion_depth + 1);
		return;
	}

	// No need to split this polygon.
	out_polygons.push(polygon.clone());
}
