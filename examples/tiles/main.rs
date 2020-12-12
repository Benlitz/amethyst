use amethyst::{
    assets::{AssetStorage, Loader},
    core::{
        math::{Point3, Vector2, Vector3},
        Named, Parent, Time, Transform, TransformBundle,
    },
    ecs::{
        Component, Entities, Entity, Join, LazyUpdate, NullStorage, Read, ReadExpect, ReadStorage,
        System, WriteStorage,
    },
    input::{is_close_requested, is_key_down, InputBundle, InputHandler, StringBindings},
    prelude::*,
    renderer::{
        camera::{ActiveCamera, Camera},
        debug_drawing::DebugLinesComponent,
        formats::texture::ImageFormat,
        palette::Srgba,
        sprite::{SpriteRender, SpriteSheet, SpriteSheetFormat, SpriteSheetHandle},
        transparent::Transparent,
        types::DefaultBackend,
        RenderDebugLines, RenderFlat2D, RenderToWindow, RenderingBundle, Texture,
    },
    tiles::{DrawTiles2DBoundsOrthoCamera, MortonEncoder, RenderTiles2D, Tile, TileMap, TileSet},
    utils::application_root_dir,
    window::ScreenDimensions,
    winit,
};

#[derive(Default)]
struct Player;

impl Component for Player {
    type Storage = NullStorage<Self>;
}

#[derive(Default)]
pub struct DrawSelectionSystem {
    start_coordinate: Option<Point3<f32>>,
}
impl<'s> System<'s> for DrawSelectionSystem {
    type SystemData = (
        Entities<'s>,
        Read<'s, ActiveCamera>,
        ReadExpect<'s, ScreenDimensions>,
        ReadStorage<'s, Camera>,
        ReadStorage<'s, Transform>,
        WriteStorage<'s, DebugLinesComponent>,
        Read<'s, InputHandler<StringBindings>>,
    );

    fn run(
        &mut self,
        (entities, active_camera, dimensions, cameras, transforms, mut debug_lines, input): Self::SystemData,
    ) {
        if let Some(lines) = (&mut debug_lines).join().next() {
            lines.clear();

            if let Some(mouse_position) = input.mouse_position() {
                let mut camera_join = (&cameras, &transforms).join();
                if let Some((camera, camera_transform)) = active_camera
                    .entity
                    .and_then(|a| camera_join.get(a, &entities))
                    .or_else(|| camera_join.next())
                {
                    let action_down = input
                        .action_is_down("select")
                        .expect("selection action missing");
                    if action_down && self.start_coordinate.is_none() {
                        // Starting a new selection
                        self.start_coordinate = Some(Point3::new(
                            mouse_position.0,
                            mouse_position.1,
                            camera_transform.translation().z,
                        ));
                    } else if action_down && self.start_coordinate.is_some() {
                        // Active drag
                        let screen_dimensions =
                            Vector2::new(dimensions.width(), dimensions.height());
                        let end_coordinate = Point3::new(
                            mouse_position.0,
                            mouse_position.1,
                            camera_transform.translation().z,
                        );

                        let mut start_world = camera.screen_to_world_point(
                            self.start_coordinate.expect("Wut?"),
                            screen_dimensions,
                            camera_transform,
                        );
                        let mut end_world = camera.screen_to_world_point(
                            end_coordinate,
                            screen_dimensions,
                            camera_transform,
                        );
                        start_world.z = 0.9;
                        end_world.z = 0.9;

                        lines.add_box(start_world, end_world, Srgba::new(0.5, 0.05, 0.65, 1.0));
                    } else if !action_down && self.start_coordinate.is_some() {
                        // End drag, remove
                        self.start_coordinate = None;
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
struct TileMapSpriteSheets {
    rect_tilemap_sheet_handle: SpriteSheetHandle,
    iso_tilemap_sheet_handle: SpriteSheetHandle,
    hexa_tilemap_sheet_handle: SpriteSheetHandle,
}

struct MapSwitchSystem {
    pressed: bool,
    current_tileset: TileSet,
}

impl Default for MapSwitchSystem {
    fn default() -> Self {
        Self {
            pressed: false,
            current_tileset: TileSet::Rectangular,
        }
    }
}
impl<'s> System<'s> for MapSwitchSystem {
    type SystemData = (
        Entities<'s>,
        Read<'s, LazyUpdate>,
        ReadExpect<'s, TileMapSpriteSheets>,
        ReadStorage<'s, TileMap<ExampleTile, MortonEncoder>>,
        Read<'s, InputHandler<StringBindings>>,
    );

    fn run(&mut self, (entities, lazy, tilemap_sprite_sheets, maps, input): Self::SystemData) {
        if input.action_is_down("swap_map").unwrap() {
            self.pressed = true;
        }
        if self.pressed && !input.action_is_down("swap_map").unwrap() {
            self.pressed = false;
            // Lazily delete the old tile map
            let mut map_join = (&entities, &maps).join();
            let (old_map_entity, _) = map_join.next().unwrap();
            let tilemap_sprite_sheets = tilemap_sprite_sheets.clone();
            let next_tileset = match self.current_tileset {
                TileSet::Rectangular => TileSet::Isometric,
                TileSet::Isometric => TileSet::Hexagonal(0),
                TileSet::Hexagonal(_) => TileSet::Rectangular,
            };
            self.current_tileset = next_tileset.clone();
            lazy.exec_mut(move |w| {
                w.delete_entity(old_map_entity).unwrap();
                init_tilemap(w, &tilemap_sprite_sheets, next_tileset);
            });
        }
    }
}
pub struct CameraSwitchSystem {
    pressed: bool,
    perspective: bool,
}
impl Default for CameraSwitchSystem {
    fn default() -> Self {
        Self {
            pressed: false,
            perspective: false,
        }
    }
}
impl<'s> System<'s> for CameraSwitchSystem {
    type SystemData = (
        Entities<'s>,
        Read<'s, LazyUpdate>,
        Read<'s, ActiveCamera>,
        ReadExpect<'s, ScreenDimensions>,
        ReadStorage<'s, Camera>,
        ReadStorage<'s, Transform>,
        ReadStorage<'s, Parent>,
        Read<'s, InputHandler<StringBindings>>,
    );

    fn run(
        &mut self,
        (entities, lazy, active_camera, dimensions, cameras, transforms, parents, input): Self::SystemData,
    ) {
        if input.action_is_down("camera_switch").unwrap() {
            self.pressed = true;
        }
        if self.pressed && !input.action_is_down("camera_switch").unwrap() {
            self.pressed = false;

            // Lazily delete the old camera
            let mut camera_join = (&entities, &cameras, &transforms, &parents).join();
            let (old_camera_entity, _, _, old_parent) = active_camera
                .entity
                .and_then(|a| camera_join.get(a, &entities))
                .or_else(|| camera_join.next())
                .unwrap();
            let old_camera_entity = old_camera_entity;

            let new_parent = old_parent.entity;

            self.perspective = !self.perspective;
            let (new_camera, new_position) = if self.perspective {
                (
                    Camera::standard_3d(dimensions.width(), dimensions.height()),
                    Vector3::new(0.0, 0.0, 500.1),
                )
            } else {
                (
                    Camera::standard_2d(dimensions.width(), dimensions.height()),
                    Vector3::new(0.0, 0.0, 1.1),
                )
            };

            lazy.exec_mut(move |w| {
                let new_camera =
                    init_camera(w, new_parent, Transform::from(new_position), new_camera);

                w.fetch_mut::<ActiveCamera>().entity = Some(new_camera);

                w.delete_entity(old_camera_entity).unwrap();
            });
        }
    }
}

#[derive(Default)]
pub struct CameraMovementSystem;
impl<'s> System<'s> for CameraMovementSystem {
    type SystemData = (
        Read<'s, ActiveCamera>,
        Entities<'s>,
        ReadStorage<'s, Camera>,
        WriteStorage<'s, Transform>,
        Read<'s, InputHandler<StringBindings>>,
    );

    fn run(&mut self, (active_camera, entities, cameras, mut transforms, input): Self::SystemData) {
        let x_move = input.axis_value("camera_x").unwrap();
        let y_move = input.axis_value("camera_y").unwrap();
        let z_move = input.axis_value("camera_z").unwrap();
        let z_move_scale = input.axis_value("camera_scale").unwrap();

        if x_move != 0.0 || y_move != 0.0 || z_move != 0.0 || z_move_scale != 0.0 {
            let mut camera_join = (&cameras, &mut transforms).join();
            if let Some((_, camera_transform)) = active_camera
                .entity
                .and_then(|a| camera_join.get(a, &entities))
                .or_else(|| camera_join.next())
            {
                camera_transform.prepend_translation_x(x_move * 5.0);
                camera_transform.prepend_translation_y(y_move * 5.0);
                camera_transform.prepend_translation_z(z_move);

                let z_scale = 0.01 * z_move_scale;
                let scale = camera_transform.scale();
                let scale = Vector3::new(scale.x + z_scale, scale.y + z_scale, scale.z + z_scale);
                camera_transform.set_scale(scale);
            }
        }
    }
}

struct MapMovementSystem {
    rotate: bool,
    translate: bool,
    vector: Vector3<f32>,
}
impl Default for MapMovementSystem {
    fn default() -> Self {
        Self {
            rotate: false,
            translate: false,
            vector: Vector3::new(100.0, 0.0, 0.0),
        }
    }
}
impl<'s> System<'s> for MapMovementSystem {
    type SystemData = (
        Read<'s, Time>,
        WriteStorage<'s, Transform>,
        ReadStorage<'s, TileMap<ExampleTile, MortonEncoder>>,
        Read<'s, InputHandler<StringBindings>>,
    );

    fn run(&mut self, (time, mut transforms, tilemaps, input): Self::SystemData) {
        if input.action_is_down("toggle_rotation").unwrap() {
            self.rotate ^= true;
        }
        if input.action_is_down("toggle_translation").unwrap() {
            self.translate ^= true;
        }
        if self.rotate {
            for (_, transform) in (&tilemaps, &mut transforms).join() {
                transform.rotate_2d(time.delta_seconds());
            }
        }
        if self.translate {
            for (_, transform) in (&tilemaps, &mut transforms).join() {
                transform.prepend_translation(self.vector * time.delta_seconds());
                if transform.translation().x > 500.0 {
                    self.vector = Vector3::new(-100.0, 0.0, 0.0);
                } else if transform.translation().x < -500.0 {
                    self.vector = Vector3::new(100.0, 0.0, 0.0);
                }
            }
        }
    }
}

fn load_sprite_sheet(world: &mut World, png_path: &str, ron_path: &str) -> SpriteSheetHandle {
    let texture_handle = {
        let loader = world.read_resource::<Loader>();
        let texture_storage = world.read_resource::<AssetStorage<Texture>>();
        loader.load(png_path, ImageFormat::default(), (), &texture_storage)
    };
    let loader = world.read_resource::<Loader>();
    let sprite_sheet_store = world.read_resource::<AssetStorage<SpriteSheet>>();
    loader.load(
        ron_path,
        SpriteSheetFormat(texture_handle),
        (),
        &sprite_sheet_store,
    )
}

// Initialize a sprite as a reference point at a fixed location
fn init_reference_sprite(world: &mut World, sprite_sheet: &SpriteSheetHandle) -> Entity {
    let mut transform = Transform::default();
    transform.set_translation_xyz(0.0, 0.0, 0.1);
    let sprite = SpriteRender::new(sprite_sheet.clone(), 0);
    world
        .create_entity()
        .with(transform)
        .with(sprite)
        .with(Transparent)
        .named("reference")
        .build()
}

// Initialize a sprite as a reference point
fn init_screen_reference_sprite(world: &mut World, sprite_sheet: &SpriteSheetHandle) -> Entity {
    let mut transform = Transform::default();
    transform.set_translation_xyz(-250.0, -245.0, 0.1);
    let sprite = SpriteRender::new(sprite_sheet.clone(), 0);
    world
        .create_entity()
        .with(transform)
        .with(sprite)
        .with(Transparent)
        .named("screen_reference")
        .build()
}

fn init_player(world: &mut World, sprite_sheet: &SpriteSheetHandle) -> Entity {
    let mut transform = Transform::default();
    transform.set_translation_xyz(0.0, 0.0, 0.1);
    let sprite = SpriteRender::new(sprite_sheet.clone(), 1);
    world
        .create_entity()
        .with(transform)
        .with(Player)
        .with(sprite)
        .with(Transparent)
        .named("player")
        .build()
}

fn init_camera(world: &mut World, parent: Entity, transform: Transform, camera: Camera) -> Entity {
    world
        .create_entity()
        .with(transform)
        .with(Parent { entity: parent })
        .with(camera)
        .named("camera")
        .build()
}

fn init_tilemap(world: &mut World, spritesheets: &TileMapSpriteSheets, tileset: TileSet) {
    let tilemap = match tileset {
        TileSet::Rectangular => TileMap::<ExampleTile, MortonEncoder>::new(
            Vector3::new(1000, 1000, 1),
            Vector3::new(20, 20, 1),
            Some(spritesheets.rect_tilemap_sheet_handle.clone()),
            TileSet::Rectangular,
        ),
        TileSet::Isometric => TileMap::<ExampleTile, MortonEncoder>::new(
            Vector3::new(30, 30, 1),
            Vector3::new(56, 29, 1),
            Some(spritesheets.iso_tilemap_sheet_handle.clone()),
            TileSet::Isometric,
        ),
        TileSet::Hexagonal(_) => TileMap::<ExampleTile, MortonEncoder>::new(
            Vector3::new(30, 30, 1),
            Vector3::new(47, 29, 1),
            Some(spritesheets.hexa_tilemap_sheet_handle.clone()),
            TileSet::Hexagonal(17),
        ),
    };
    world
        .create_entity()
        .with(tilemap)
        .with(Transform::default())
        .build();
}

#[derive(Default, Clone)]
struct ExampleTile;
impl Tile for ExampleTile {
    fn sprite(&self, p: Point3<u32>, _: &World) -> Option<usize> {
        Some(((p.x + p.y) % 2) as usize)
    }
}

#[derive(Default)]
struct Example;
impl SimpleState for Example {
    fn on_start(&mut self, data: StateData<'_, GameData<'_, '_>>) {
        let world = data.world;
        world.register::<Named>();
        world.register::<Player>();

        let circle_sprite_sheet_handle = load_sprite_sheet(
            world,
            "texture/Circle_Spritesheet.png",
            "texture/Circle_Spritesheet.ron",
        );

        let tilemap_sprite_sheets = TileMapSpriteSheets {
            rect_tilemap_sheet_handle: load_sprite_sheet(
                world,
                "texture/cp437_20x20.png",
                "texture/cp437_20x20.ron",
            ),
            iso_tilemap_sheet_handle: load_sprite_sheet(
                world,
                "texture/Isometric_tiles.png",
                "texture/Isometric_tiles.ron",
            ),
            hexa_tilemap_sheet_handle: load_sprite_sheet(
                world,
                "texture/Hexagonal_tiles.png",
                "texture/Hexagonal_tiles.ron",
            ),
        };

        let (width, height) = {
            let dim = world.read_resource::<ScreenDimensions>();
            (dim.width(), dim.height())
        };

        let _reference = init_reference_sprite(world, &circle_sprite_sheet_handle);
        let player = init_player(world, &circle_sprite_sheet_handle);
        let camera = init_camera(
            world,
            player,
            Transform::from(Vector3::new(0.0, 0.0, 1.1)),
            Camera::standard_2d(width, height),
        );
        world.fetch_mut::<ActiveCamera>().entity = Some(camera);
        let _reference_screen = init_screen_reference_sprite(world, &circle_sprite_sheet_handle);

        // create a test debug lines entity
        let _ = world
            .create_entity()
            .with(DebugLinesComponent::with_capacity(1))
            .build();

        init_tilemap(world, &tilemap_sprite_sheets, TileSet::Rectangular);
        world.insert(tilemap_sprite_sheets);
    }

    fn handle_event(
        &mut self,
        data: StateData<'_, GameData<'_, '_>>,
        event: StateEvent,
    ) -> SimpleTrans {
        let StateData { .. } = data;
        if let StateEvent::Window(event) = &event {
            if is_close_requested(&event) || is_key_down(&event, winit::VirtualKeyCode::Escape) {
                Trans::Quit
            } else {
                Trans::None
            }
        } else {
            Trans::None
        }
    }
}

fn main() -> amethyst::Result<()> {
    println!("START");
    amethyst::Logger::from_config(Default::default())
        .level_for("amethyst_tiles", log::LevelFilter::Warn)
        .start();

    let app_root = application_root_dir()?;
    let assets_directory = app_root.join("examples/tiles/assets");
    let display_config_path = app_root.join("examples/tiles/config/display.ron");

    let game_data = GameDataBuilder::default()
        .with_bundle(TransformBundle::new())
        .unwrap()
        .with_bundle(
            InputBundle::<StringBindings>::new()
                .with_bindings_from_file("examples/tiles/config/input.ron")
                .unwrap(),
        )
        .unwrap()
        .with(
            MapSwitchSystem::default(),
            "MapSwitchSystem",
            &["input_system"],
        )
        .with(
            MapMovementSystem::default(),
            "MapMovementSystem",
            &["input_system"],
        )
        .with(
            CameraSwitchSystem::default(),
            "camera_switch",
            &["input_system"],
        )
        .with(
            CameraMovementSystem::default(),
            "movement",
            &["camera_switch"],
        )
        .with(
            DrawSelectionSystem::default(),
            "DrawSelectionSystem",
            &["camera_switch"],
        )
        .with_bundle(
            RenderingBundle::<DefaultBackend>::new()
                .with_plugin(
                    RenderToWindow::from_config_path(display_config_path)?
                        .with_clear([0.34, 0.36, 0.52, 1.0]),
                )
                .with_plugin(RenderDebugLines::default())
                .with_plugin(RenderFlat2D::default())
                .with_plugin(RenderTiles2D::<
                    ExampleTile,
                    MortonEncoder,
                    DrawTiles2DBoundsOrthoCamera,
                >::default()),
        )
        .unwrap();

    let mut game = Application::build(assets_directory, Example)?.build(game_data)?;
    game.run();
    Ok(())
}
