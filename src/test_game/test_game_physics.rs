use rapier3d::prelude as r3d;
use square_wheel_lib::common::{bbox::*, bsp_map_compact, math_types::*};
use std::sync::Arc;

pub struct TestGamePhysics
{
	map: Arc<bsp_map_compact::BSPMap>,

	rigid_body_set: r3d::RigidBodySet,
	collider_set: r3d::ColliderSet,

	physics_pipeline: r3d::PhysicsPipeline,
	island_manager: r3d::IslandManager,
	broad_phase: r3d::BroadPhase,
	narrow_phase: r3d::NarrowPhase,
	impulse_joint_set: r3d::ImpulseJointSet,
	multibody_joint_set: r3d::MultibodyJointSet,
	ccd_solver: r3d::CCDSolver,
	hooks: PhysicsHooks,
	event_handler: (),
}

pub type ObjectHandle = r3d::RigidBodyHandle;

impl TestGamePhysics
{
	pub fn new(map: Arc<bsp_map_compact::BSPMap>) -> Self
	{
		let rigid_body_set = r3d::RigidBodySet::new();
		let mut collider_set = r3d::ColliderSet::new();

		// Static geometry.
		collider_set.insert(make_map_collider(&map));

		Self {
			map,
			rigid_body_set,
			collider_set,
			physics_pipeline: r3d::PhysicsPipeline::new(),
			island_manager: r3d::IslandManager::new(),
			broad_phase: r3d::BroadPhase::new(),
			narrow_phase: r3d::NarrowPhase::new(),
			impulse_joint_set: r3d::ImpulseJointSet::new(),
			multibody_joint_set: r3d::MultibodyJointSet::new(),
			ccd_solver: r3d::CCDSolver::new(),
			hooks: PhysicsHooks::new(),
			event_handler: (),
		}
	}

	pub fn add_object(&mut self, position: &Vec3f, rotation: &QuaternionF, bbox: &BBox) -> ObjectHandle
	{
		// TODO - maybe tune physics and disable CCD?
		let body = r3d::RigidBodyBuilder::dynamic()
			.translation(r3d::Vector::new(position.x, position.y, position.z))
			.rotation(quaternion_to_ang_vector(rotation))
			.ccd_enabled(true)
			.build();

		let bbox_half_size = bbox.get_size() * 0.5;
		let bbox_center = bbox.get_center();

		let collider = r3d::ColliderBuilder::cuboid(bbox_half_size.x, bbox_half_size.y, bbox_half_size.z)
			.translation(r3d::Vector::new(bbox_center.x, bbox_center.y, bbox_center.z))
			.restitution(0.5)
			.friction(0.5)
			.build();

		let handle = self.rigid_body_set.insert(body);
		self.collider_set
			.insert_with_parent(collider, handle, &mut self.rigid_body_set);

		handle
	}

	pub fn add_submodel_object(&mut self, submodel_index: usize, shift: &Vec3f, rotation: &QuaternionF)
		-> ObjectHandle
	{
		let submodel = &self.map.submodels[submodel_index];
		let bbox = bsp_map_compact::get_submodel_bbox(&self.map, submodel);
		let bbox_half_size = bbox.get_size() * 0.5;
		let bbox_center = bbox.get_center();

		let position = shift + bbox_center;

		let body = r3d::RigidBodyBuilder::kinematic_position_based()
			.translation(r3d::Vector::new(position.x, position.y, position.z))
			.rotation(quaternion_to_ang_vector(rotation))
			.ccd_enabled(true)
			.build();

		let collider = r3d::ColliderBuilder::cuboid(bbox_half_size.x, bbox_half_size.y, bbox_half_size.z)
			.restitution(0.0)
			.build();

		let handle = self.rigid_body_set.insert(body);
		self.collider_set
			.insert_with_parent(collider, handle, &mut self.rigid_body_set);

		handle
	}

	pub fn add_character_object(&mut self, position: &Vec3f, width: f32, heigt: f32) -> ObjectHandle
	{
		// TODO - maybe tune physics and disable CCD?
		let body = r3d::RigidBodyBuilder::dynamic()
			.translation(r3d::Vector::new(position.x, position.y, position.z))
			.ccd_enabled(true)
			.linear_damping(0.5)
			.lock_rotations()
			.build();

		let collider = r3d::ColliderBuilder::capsule_z((heigt - width) * 0.5, width * 0.5)
			.restitution(0.0)
			.friction(0.95)
			.active_hooks(r3d::ActiveHooks::MODIFY_SOLVER_CONTACTS)
			.user_data(CHARACTER_USER_DATA)
			.build();

		let handle = self.rigid_body_set.insert(body);
		self.collider_set
			.insert_with_parent(collider, handle, &mut self.rigid_body_set);

		handle
	}

	pub fn get_object_velocity(&self, handle: ObjectHandle) -> Vec3f
	{
		let body = &self.rigid_body_set[handle];
		let velocity = body.linvel();
		Vec3f::new(velocity.x, velocity.y, velocity.z)
	}

	pub fn add_object_velocity(&mut self, handle: ObjectHandle, velocity: &Vec3f)
	{
		let body = &mut self.rigid_body_set[handle];
		let impulse = velocity * body.mass();
		body.apply_impulse(r3d::Vector::new(impulse.x, impulse.y, impulse.z), true);
	}

	pub fn teleport_object(&mut self, handle: ObjectHandle, position: &Vec3f)
	{
		let body = &mut self.rigid_body_set[handle];
		body.set_translation(r3d::Vector::new(position.x, position.y, position.z), true)
	}

	pub fn set_kinematic_object_position(&mut self, handle: ObjectHandle, position: &Vec3f)
	{
		let body = &mut self.rigid_body_set[handle];
		body.set_next_kinematic_translation(r3d::Vector::new(position.x, position.y, position.z))
	}

	pub fn is_object_on_ground(&self, handle: ObjectHandle) -> bool
	{
		let body = &self.rigid_body_set[handle];
		for &collider in body.colliders()
		{
			for contact in self.narrow_phase.contacts_with(collider)
			{
				if contact.collider1 == collider
				{
					for manifold in &contact.manifolds
					{
						if manifold.data.normal.z < -0.8
						{
							return true;
						}
					}
				}
				if contact.collider2 == collider
				{
					for manifold in &contact.manifolds
					{
						if manifold.data.normal.z > 0.8
						{
							return true;
						}
					}
				}
			}
		}

		false
	}

	pub fn remove_object(&mut self, handle: ObjectHandle)
	{
		self.rigid_body_set.remove(
			handle,
			&mut self.island_manager,
			&mut self.collider_set,
			&mut self.impulse_joint_set,
			&mut self.multibody_joint_set,
			true,
		);
	}

	pub fn get_object_location(&self, handle: ObjectHandle) -> (Vec3f, QuaternionF)
	{
		let body = &self.rigid_body_set[handle];
		let position = body.position();
		let translation = position.translation;
		let rotation = position.rotation;
		let rotation_xyzw = rotation.coords;

		(
			Vec3f::new(translation.x, translation.y, translation.z),
			QuaternionF::from_sv(
				rotation_xyzw[3],
				Vec3f::new(rotation_xyzw[0], rotation_xyzw[1], rotation_xyzw[2]),
			),
		)
	}

	pub fn update(&mut self, time_delta_s: f32)
	{
		let gravity = r3d::Vector::new(0.0, 0.0, -627.84);

		// Perform several physics steps in case of low FPS.
		let max_dt = 1.0 / 30.0;
		let mut cur_step_time = 0.0;
		while cur_step_time < time_delta_s
		{
			let cur_dt = (time_delta_s - cur_step_time).min(max_dt);
			cur_step_time += max_dt;

			let mut integration_parameters = r3d::IntegrationParameters::default();
			integration_parameters.dt = cur_dt;

			self.physics_pipeline.step(
				&gravity,
				&integration_parameters,
				&mut self.island_manager,
				&mut self.broad_phase,
				&mut self.narrow_phase,
				&mut self.rigid_body_set,
				&mut self.collider_set,
				&mut self.impulse_joint_set,
				&mut self.multibody_joint_set,
				&mut self.ccd_solver,
				&self.hooks,
				&self.event_handler,
			);
		}
	}
}

fn make_map_collider(map: &bsp_map_compact::BSPMap) -> r3d::Collider
{
	let vertices = map
		.vertices
		.iter()
		.map(|v| r3d::Point::new(v.x, v.y, v.z))
		.collect::<Vec<_>>();

	let mut indices = Vec::new();
	for leaf in &map.leafs
	{
		for polygon in &map.polygons[leaf.first_polygon as usize .. (leaf.first_polygon + leaf.num_polygons) as usize]
		{
			// TODO - ignore polygons without collisions.
			for i in 0 .. polygon.num_vertices - 2
			{
				indices.push([
					polygon.first_vertex + 0,
					polygon.first_vertex + i + 1,
					polygon.first_vertex + i + 2,
				]);
			}
		}
	}

	r3d::ColliderBuilder::trimesh(vertices, indices).build()
}

// Convert quaternion to normalized rotation axis scaled by angle.
fn quaternion_to_ang_vector(quat: &QuaternionF) -> r3d::AngVector<r3d::Real>
{
	let quat_v_magnitude = quat.v.magnitude();
	if quat_v_magnitude == 0.0
	{
		return r3d::AngVector::new(0.0, 0.0, 0.0);
	}

	let axis = quat.v / quat_v_magnitude;

	let angle = 2.0 * quat_v_magnitude.atan2(quat.s);

	let axis_angle_scaled = axis * angle;

	r3d::AngVector::new(axis_angle_scaled.x, axis_angle_scaled.y, axis_angle_scaled.z)
}

struct PhysicsHooks {}

impl PhysicsHooks
{
	fn new() -> Self
	{
		Self {}
	}
}

const CHARACTER_USER_DATA: u128 = 42;
const STAIRS_HACK_NORMAL_Z: f32 = 0.3;

impl r3d::PhysicsHooks for PhysicsHooks
{
	fn modify_solver_contacts(&self, context: &mut r3d::ContactModificationContext)
	{
		// For colliders with stairs hack modify contact point normal in order to avoid slowing-down while climbing stairs.
		let collider1 = &context.colliders[context.collider1];
		if collider1.user_data == CHARACTER_USER_DATA
		{
			if context.normal.z < -STAIRS_HACK_NORMAL_Z
			{
				*context.normal = r3d::Vector::new(0.0, 0.0, -1.0);
			}
		}

		let collider2 = &context.colliders[context.collider2];
		if collider2.user_data == CHARACTER_USER_DATA
		{
			if context.normal.z > STAIRS_HACK_NORMAL_Z
			{
				*context.normal = r3d::Vector::new(0.0, 0.0, 1.0);
			}
		}

		// Modify also friction to disable walls friction.
		if collider1.user_data == CHARACTER_USER_DATA || collider2.user_data == CHARACTER_USER_DATA
		{
			let fiction_scale = context.normal.z.abs();
			for contact in context.solver_contacts.iter_mut()
			{
				contact.friction *= fiction_scale;
			}
		}
	}
}
