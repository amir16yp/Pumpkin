use rand::RngExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering::Relaxed};

use crate::block::blocks::fire::FireBlockBase;
use crate::block::blocks::fire::fire::FireBlock;
use crate::entity::{
    Entity, EntityBase, EntityBaseFuture, NBTStorage, living::LivingEntity, player::Player,
};
use crate::server::Server;
use crate::world::World;
use pumpkin_data::damage::DamageType;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_util::Difficulty;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

pub struct LightningBoltEntity {
    entity: Entity,
    life: AtomicI32,
    flashes: AtomicI32,
    visual_only: AtomicBool,
    cause: tokio::sync::Mutex<Option<Arc<Player>>>,
    // Store hit entity IDs as i32 to avoid reference cycles or holding too many Arcs
    hit_entities: tokio::sync::Mutex<std::collections::HashSet<i32>>,
    random_interval_offset: i32,
}

impl LightningBoltEntity {
    pub fn new(entity: Entity) -> Self {
        let mut rng = rand::rng();
        Self {
            entity,
            life: AtomicI32::new(2),
            flashes: AtomicI32::new(rng.random_range(1..=3)),
            visual_only: AtomicBool::new(false),
            cause: tokio::sync::Mutex::new(None),
            hit_entities: tokio::sync::Mutex::new(std::collections::HashSet::new()),
            random_interval_offset: rng.random_range(0..10),
        }
    }

    pub fn set_visual_only(&self, visual_only: bool) {
        self.visual_only.store(visual_only, Relaxed);
    }

    pub async fn set_cause(&self, cause: Option<Arc<Player>>) {
        *self.cause.lock().await = cause;
    }

    pub async fn get_cause(&self) -> Option<Arc<Player>> {
        self.cause.lock().await.clone()
    }

    async fn spawn_fire(&self, world: &Arc<World>, pos: Vector3<f64>, additional_sources: i32) {
        if self.visual_only.load(Relaxed) {
            return;
        }
        let block_pos = BlockPos::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        );

        if world.is_loaded(&block_pos) {
            let fire_block = FireBlockBase::get_fire_type(world, &block_pos);
            if FireBlockBase::can_place_at(world, &block_pos) {
                let state_id = FireBlock.get_state_for_position(world, &fire_block, &block_pos);
                world
                    .set_block_state(&block_pos, state_id, BlockFlags::NOTIFY_ALL)
                    .await;
            }

            let offsets: Vec<Vector3<i32>> = {
                let mut rng = rand::rng();
                (0..additional_sources)
                    .map(|_| {
                        Vector3::new(
                            rng.random_range(-1..=1),
                            rng.random_range(-1..=1),
                            rng.random_range(-1..=1),
                        )
                    })
                    .collect()
            };
            for offset in offsets {
                let offset_pos = block_pos.offset(offset);
                if world.is_loaded(&offset_pos) {
                    let fire_block = FireBlockBase::get_fire_type(world, &offset_pos);
                    if FireBlockBase::can_place_at(world, &offset_pos) {
                        let state_id =
                            FireBlock.get_state_for_position(world, &fire_block, &offset_pos);
                        world
                            .set_block_state(&offset_pos, state_id, BlockFlags::NOTIFY_ALL)
                            .await;
                    }
                }
            }
        }
    }
}

impl NBTStorage for LightningBoltEntity {}

impl EntityBase for LightningBoltEntity {
    fn tick<'a>(
        &'a self,
        caller: &'a Arc<dyn EntityBase>,
        server: &'a Server,
    ) -> EntityBaseFuture<'a, ()> {
        Box::pin(async move {
            self.entity.tick(caller, server).await;

            let world = self.entity.world.load();
            let pos = self.entity.pos.load();

            let life = self.life.load(Relaxed);
            if life == 2 {
                // Play weather sounds on first strike
                world.play_sound_fine(
                    Sound::EntityLightningBoltThunder,
                    SoundCategory::Weather,
                    &pos,
                    10000.0,
                    0.8 + rand::random::<f32>() * 0.2,
                );
                world.play_sound_fine(
                    Sound::EntityLightningBoltImpact,
                    SoundCategory::Weather,
                    &pos,
                    2.0,
                    0.5 + rand::random::<f32>() * 0.2,
                );

                let difficulty = world.level_info.load().difficulty;
                if difficulty == Difficulty::Normal || difficulty == Difficulty::Hard {
                    self.spawn_fire(&world, pos, 4).await;
                }
            }

            let next_life = life - 1;
            self.life.store(next_life, Relaxed);

            if next_life < 0 {
                let flashes = self.flashes.load(Relaxed);
                if flashes == 0 {
                    self.entity.remove().await;
                } else if next_life < -self.random_interval_offset {
                    self.flashes.store(flashes - 1, Relaxed);
                    self.life.store(1, Relaxed);
                    self.spawn_fire(&world, pos, 0).await;
                }
            }

            if next_life >= 0 && !self.visual_only.load(Relaxed) {
                let aabb = self.entity.bounding_box.load().expand(3.0, 3.0, 3.0);
                let entities = world.get_all_at_box(&aabb);

                let mut hit_guard = self.hit_entities.lock().await;
                let lightning_bolt_entity_base = caller; // Caller is the Arc<dyn EntityBase> of this entity

                for target in entities {
                    let target_id = target.get_entity().entity_id;
                    if target_id != self.entity.entity_id && !hit_guard.contains(&target_id) {
                        target.get_entity().set_on_fire_for(8.0);
                        target
                            .damage(
                                lightning_bolt_entity_base.as_ref(),
                                5.0,
                                DamageType::LIGHTNING_BOLT,
                            )
                            .await;
                        hit_guard.insert(target_id);
                    }
                }
            }
        })
    }

    fn get_entity(&self) -> &Entity {
        &self.entity
    }

    fn get_living_entity(&self) -> Option<&LivingEntity> {
        None
    }

    fn as_nbt_storage(&self) -> &dyn NBTStorage {
        self
    }

    fn cast_any(&self) -> &dyn std::any::Any {
        self
    }
}
