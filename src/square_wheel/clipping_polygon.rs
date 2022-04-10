use common::math_types::*;

// 2d cLipping polygon. Has small number of fixed sizes.
#[derive(Copy, Clone)]
pub struct ClippingPolygon
{
	// Now it is just an axis-aligned box.
	x: ClipAxis,
	y: ClipAxis,
}

impl ClippingPolygon
{
	pub fn from_box(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Self
	{
		Self {
			x: ClipAxis { min: min_x, max: max_x },
			y: ClipAxis { min: min_y, max: max_y },
		}
	}

	pub fn from_point(point: &Vec2f) -> Self
	{
		Self {
			x: ClipAxis {
				min: point.x,
				max: point.x,
			},
			y: ClipAxis {
				min: point.y,
				max: point.y,
			},
		}
	}

	pub fn is_empty_or_invalid(&self) -> bool
	{
		!self.is_valid_and_non_empty()
	}

	pub fn is_valid_and_non_empty(&self) -> bool
	{
		self.x.min < self.x.max && self.y.min < self.y.max
	}

	// Result polygon will contain both "self" and "other".
	pub fn extend(&mut self, other: ClippingPolygon)
	{
		self.x.extend(&other.x);
		self.y.extend(&other.y);
	}

	pub fn extend_with_polygon(&mut self, polygon_points: &[Vec2f])
	{
		for point in polygon_points
		{
			self.extend_with_point(point);
		}
	}

	pub fn extend_with_point(&mut self, point: &Vec2f)
	{
		self.x.extend_with_point(point.x);
		self.y.extend_with_point(point.y);
	}

	// Both "self" and "other" will contain result polygon.
	pub fn intersect(&mut self, other: ClippingPolygon)
	{
		self.x.intersect(&other.x);
		self.y.intersect(&other.y);
	}

	// Input polygon must be non-empty.
	pub fn intersect_with_polygon(&mut self, polygon_points: &[Vec2f])
	{
		let mut points_bound = Self::from_point(&polygon_points[0]);
		for point in &polygon_points[ 1 .. ]
		{
			points_bound.extend_with_point(point);
		}

		self.intersect(points_bound);
	}

	pub fn get_clip_planes(&self) -> [Vec3f; 4]
	{
		[
			Vec3f::new(-1.0, 0.0, -self.x.max),
			Vec3f::new(1.0, 0.0, -self.x.min),
			Vec3f::new(0.0, -1.0, -self.y.max),
			Vec3f::new(0.0, 1.0, -self.y.min),
		]
	}
}

#[derive(Copy, Clone)]
struct ClipAxis
{
	min: f32,
	max: f32,
}

impl ClipAxis
{
	fn extend(&mut self, other: &ClipAxis)
	{
		if other.min < self.min
		{
			self.min = other.min;
		}
		if other.max > self.max
		{
			self.max = other.max;
		}
	}

	fn extend_with_point(&mut self, point: f32)
	{
		if point < self.min
		{
			self.min = point;
		}
		if point > self.max
		{
			self.max = point;
		}
	}

	fn intersect(&mut self, other: &ClipAxis)
	{
		if other.min > self.min
		{
			self.min = other.min;
		}
		if other.max < self.max
		{
			self.max = other.max;
		}
	}
}
