use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::{Controls, Goal, GoalFuture};
use crate::entity::EntityBase;
use crate::entity::mob::Mob;
use pumpkin_util::math::vector3::Vector3;
use rand::RngExt;

pub struct LeapAtTargetGoal {
    goal_control: Controls,
    yd: f64,
    target: Option<Arc<dyn EntityBase>>,
}

impl LeapAtTargetGoal {
    #[must_use]
    pub fn new(yd: f64) -> Self {
        Self {
            goal_control: Controls::MOVE | Controls::JUMP,
            yd,
            target: None,
        }
    }
}

impl Goal for LeapAtTargetGoal {
    fn can_start<'a>(&'a mut self, mob: &'a dyn Mob) -> GoalFuture<'a, bool> {
        Box::pin(async move {
            if mob.get_entity().has_passengers().await {
                return false;
            }

            let target_lock = mob.get_mob_entity().target.lock().await;
            let Some(target) = target_lock.as_ref() else {
                return false;
            };

            let mob_pos = mob.get_entity().pos.load();
            let target_pos = target.get_entity().pos.load();
            let d = mob_pos.squared_distance_to_vec(&target_pos);

            if d < 4.0 || d > 16.0 {
                return false;
            }

            if !mob.get_entity().on_ground.load(Ordering::Relaxed) {
                return false;
            }

            if mob.get_random().random_range(0..5) != 0 {
                return false;
            }

            self.target = Some(target.clone());
            true
        })
    }

    fn should_continue<'a>(&'a self, mob: &'a dyn Mob) -> GoalFuture<'a, bool> {
        Box::pin(async move { !mob.get_entity().on_ground.load(Ordering::Relaxed) })
    }

    fn start<'a>(&'a mut self, mob: &'a dyn Mob) -> GoalFuture<'a, ()> {
        Box::pin(async move {
            let Some(target) = self.target.as_ref() else {
                return;
            };

            let target_pos = target.get_entity().pos.load();
            let mob_pos = mob.get_entity().pos.load();
            let movement = mob.get_entity().velocity.load();

            let mut delta = Vector3::new(target_pos.x - mob_pos.x, 0.0, target_pos.z - mob_pos.z);
            if delta.length_squared() > 1.0e-7 {
                delta = delta.normalize() * 0.4 + movement * 0.2;
            }

            mob.get_entity()
                .set_velocity(Vector3::new(delta.x, self.yd, delta.z));
        })
    }

    fn stop<'a>(&'a mut self, _mob: &'a dyn Mob) -> GoalFuture<'a, ()> {
        Box::pin(async move {
            self.target = None;
        })
    }

    fn controls(&self) -> Controls {
        self.goal_control
    }
}
