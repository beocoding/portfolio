use bevy::{
    color::palettes::basic::*,
    input_focus::{
        tab_navigation::{TabGroup, TabIndex, TabNavigationPlugin},
        InputDispatchPlugin,
    },
    picking::hover::Hovered,
    prelude::*,
};
use bevy_ui_widgets::{observe, Slider, SliderRange, SliderThumb, SliderValue, UiWidgetsPlugins, ValueChange};

use crate::{
    config::{ChunkSettings, NoiseSettings},
    noise::{TerrainMeshlet, TerrainTileId},
    pipeline::{recalculate_octave_fractals, recalculate_terrain},
};

const SLIDER_TRACK: Color = Color::srgb(0.05, 0.05, 0.05);
const SLIDER_THUMB: Color = Color::srgb(0.35, 0.75, 0.35);

// Marker components to identify which parameter each slider controls
#[derive(Component)]
struct OffsetXSlider;

#[derive(Component)]
struct OffsetYSlider;

#[derive(Component)]
struct OctavesSlider;

#[derive(Component)]
struct SeedSlider;

#[derive(Component)]
struct NoiseScaleSlider;

#[derive(Component)]
struct LacunaritySlider;

#[derive(Component)]
struct PersistenceSlider;
#[derive(Component)]
struct HeightScaleSlider;

// Marker components to identify which parameter each label displays
#[derive(Component)]
struct OffsetXLabel;

#[derive(Component)]
struct OffsetYLabel;

#[derive(Component)]
struct OctavesLabel;

#[derive(Component)]
struct SeedLabel;

#[derive(Component)]
struct NoiseScaleLabel;

#[derive(Component)]
struct LacunarityLabel;

#[derive(Component)]
struct PersistenceLabel;
#[derive(Component)]
struct HeightScaleLabel;
#[derive(Component)]
struct TerrainSliderThumb;


pub struct TerrainUiPlugin;

impl Plugin for TerrainUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            UiWidgetsPlugins,
            InputDispatchPlugin,
            TabNavigationPlugin,
        ))
        .add_systems(Startup, setup_ui)
        .add_systems(Update, (
            update_slider_values_from_config,
            update_slider_thumbs,
        ))
        .add_systems(Update, (
            update_slider_labels, 
            recalculate_on_noise_change,
            // regenerate_terrain_texture_on_noise_change
        ));
    }
}

fn setup_ui(mut commands: Commands, asset_server: Res<AssetServer>, noise_config: Res<NoiseSettings>) {
    commands.spawn((
        Name::new("Terrain Settings Panel"),
        Node {
            width: Val::Px(300.0),
            height: Val::Auto,
            position_type: PositionType::Absolute,
            right: Val::Px(10.0),
            top: Val::Px(10.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(15.0)),
            row_gap: Val::Px(15.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.9)),
        BorderRadius::all(Val::Px(8.0)),
        TabGroup::default(),
        Children::spawn((
            // Title
            Spawn((
                Name::new("Title Text"),
                Node {
                    display: Display::Flex,
                    padding: UiRect::all(Val::Px(5.0)),
                    margin: UiRect::bottom(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.3, 0.3, 0.3, 0.5)),
                Children::spawn((
                    Spawn((
                        Text::new("Terrain Settings"),
                        TextFont {
                            font_size: 24.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    )),
                )),
            )),
            // Offset X
            Spawn(labeled_slider(
                &asset_server,
                "Offset X",
                -1000.0,
                1000.0,
                noise_config.offset.x,
                OffsetXLabel,
                OffsetXSlider,
                observe(
                    |event: On<ValueChange<f32>>, mut config: ResMut<NoiseSettings>| {
                        config.offset.x = event.value;
                    },
                ),
            )),
            // Offset Y
            Spawn(labeled_slider(
                &asset_server,
                "Offset Y",
                -1000.0,
                1000.0,
                noise_config.offset.y,
                OffsetYLabel,
                OffsetYSlider,
                observe(
                    |event: On<ValueChange<f32>>, mut config: ResMut<NoiseSettings>| {
                        config.offset.y = event.value;
                    },
                ),
            )),
            // Octaves
            Spawn(labeled_slider(
                &asset_server,
                "Octaves",
                1.0,
                8.0,
                noise_config.octaves as f32,
                OctavesLabel,
                OctavesSlider,
                observe(
                    |event: On<ValueChange<f32>>, mut config: ResMut<NoiseSettings>| {
                        config.octaves = event.value.round() as usize;
                    },
                ),
            )),
            // Seed
            Spawn(labeled_slider(
                &asset_server,
                "Seed",
                0.0,
                10000.0,
                noise_config.seed.unwrap_or(0) as f32,
                SeedLabel,
                SeedSlider,
                observe(
                    |event: On<ValueChange<f32>>, mut config: ResMut<NoiseSettings>| {
                        config.seed = Some(event.value.round() as u32);
                    },
                ),
            )),
            // Noise Scale
            Spawn(labeled_slider(
                &asset_server,
                "Noise Scale",
                0.001,
                1000.0,
                noise_config.noise_scale,
                NoiseScaleLabel,
                NoiseScaleSlider,
                observe(
                    |event: On<ValueChange<f32>>, mut config: ResMut<NoiseSettings>| {
                        config.noise_scale = event.value;
                    },
                ),
            )),
            // Lacunarity
            Spawn(labeled_slider(
                &asset_server,
                "Lacunarity",
                1.0,
                4.0,
                noise_config.lacunarity,
                LacunarityLabel,
                LacunaritySlider,
                observe(
                    |event: On<ValueChange<f32>>, mut config: ResMut<NoiseSettings>| {
                        config.lacunarity = event.value;
                    },
                ),
            )),
            // Persistence
            Spawn(labeled_slider(
                &asset_server,
                "Persistence",
                0.0,
                1.0,
                noise_config.persistence,
                PersistenceLabel,
                PersistenceSlider,
                observe(
                    |event: On<ValueChange<f32>>, mut config: ResMut<NoiseSettings>| {
                        config.persistence = event.value;
                    },
                ),
            )),
            Spawn(labeled_slider(
                &asset_server,
                "Height Scale",
                1.0,
                500.0,
                noise_config.height_scale,
                HeightScaleLabel,
                HeightScaleSlider,
                observe(
                    |event: On<ValueChange<f32>>, mut config: ResMut<NoiseSettings>| {
                        config.height_scale = event.value;
                    },
                ),
            )),
        )),
    ));
}

fn labeled_slider<LM: Component, SM: Component, O: Bundle>(
    asset_server: &AssetServer,
    label: &str,
    min: f32,
    max: f32,
    value: f32,
    label_marker: LM,
    slider_marker: SM,
    observer: O,
) -> impl Bundle {
    println!("📝 Creating slider for: {}", label);
    (
        Name::new(format!("{} Container", label)),
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(5.0),
            ..default()
        },
        Children::spawn((
            // Label text
            Spawn((
                Name::new(format!("{} Label", label)),
                Node {
                    display: Display::Flex,
                    padding: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.2, 0.2, 0.2, 0.5)), // Debug background
                Children::spawn((
                    Spawn((
                        Text::new(format!("{}: {:.3}", label, value)),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        label_marker,
                    )),
                )),
            )),
            // Slider
            Spawn((
                Name::new(format!("{} Slider", label)),
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Stretch,
                    column_gap: Val::Px(4.0),
                    height: Val::Px(12.0),
                    width: Val::Percent(100.0),
                    ..default()
                },
                Slider::default(),
                SliderValue(value),
                SliderRange::new(min, max),
                Hovered::default(),
                TabIndex(0),
                slider_marker,
                observer,
                Children::spawn((
                    // Slider background rail
                    Spawn((
                        Name::new(format!("{} Rail", label)),
                        Node {
                            height: Val::Px(6.0),
                            ..default()
                        },
                        BackgroundColor(SLIDER_TRACK),
                        BorderRadius::all(Val::Px(3.0)),
                    )),
                    // Track for thumb positioning
                    Spawn((
                        Name::new(format!("{} Track", label)),
                        Node {
                            display: Display::Flex,
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            right: Val::Px(12.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            ..default()
                        },
                        Children::spawn((
                            // Thumb
                            Spawn((
                                Name::new(format!("{} Thumb", label)),
                                TerrainSliderThumb,
                                SliderThumb,
                                Node {
                                    display: Display::Flex,
                                    width: Val::Px(12.0),
                                    height: Val::Px(12.0),
                                    position_type: PositionType::Absolute,
                                    left: Val::Percent(0.0),
                                    ..default()
                                },
                                BorderRadius::MAX,
                                BackgroundColor(SLIDER_THUMB),
                            )),
                        )),
                    )),
                )),
            )),
        )),
    )
}

// Update slider thumb positions when values change
fn update_slider_thumbs(
    sliders: Query<(Entity, &SliderValue, &SliderRange), Changed<SliderValue>>,
    children: Query<&Children>,
    mut thumbs: Query<&mut Node, With<TerrainSliderThumb>>,
) {
    for (slider_entity, value, range) in sliders.iter() {
        for child in children.iter_descendants(slider_entity) {
            if let Ok(mut thumb_node) = thumbs.get_mut(child) {
                thumb_node.left = Val::Percent(range.thumb_position(value.0) * 100.0);
            }
        }
    }
}

// Sync NoiseConfig values back to slider components
fn update_slider_values_from_config(
    noise_config: Res<NoiseSettings>,
    offset_x: Query<(Entity, &SliderValue), With<OffsetXSlider>>,
    offset_y: Query<(Entity, &SliderValue), (With<OffsetYSlider>, Without<OffsetXSlider>)>,
    octaves: Query<(Entity, &SliderValue), (With<OctavesSlider>, Without<OffsetXSlider>, Without<OffsetYSlider>)>,
    seed: Query<
        (Entity, &SliderValue),
        (
            With<SeedSlider>,
            Without<OffsetXSlider>,
            Without<OffsetYSlider>,
            Without<OctavesSlider>,
        ),
    >,
    noise_scale: Query<
        (Entity, &SliderValue),
        (
            With<NoiseScaleSlider>,
            Without<OffsetXSlider>,
            Without<OffsetYSlider>,
            Without<OctavesSlider>,
            Without<SeedSlider>,
        ),
    >,
    lacunarity: Query<
        (Entity, &SliderValue),
        (
            With<LacunaritySlider>,
            Without<OffsetXSlider>,
            Without<OffsetYSlider>,
            Without<OctavesSlider>,
            Without<SeedSlider>,
            Without<NoiseScaleSlider>,
        ),
    >,
    persistence: Query<
        (Entity, &SliderValue),
        (
            With<PersistenceSlider>,
            Without<OffsetXSlider>,
            Without<OffsetYSlider>,
            Without<OctavesSlider>,
            Without<SeedSlider>,
            Without<NoiseScaleSlider>,
            Without<LacunaritySlider>,
        ),
    >,
    height_scale: Query<
        (Entity, &SliderValue),
        (
            With<HeightScaleSlider>,
            Without<OffsetXSlider>,
            Without<OffsetYSlider>,
            Without<OctavesSlider>,
            Without<SeedSlider>,
            Without<NoiseScaleSlider>,
            Without<LacunaritySlider>,
            Without<PersistenceSlider>,

        ),
    >,
    mut commands: Commands,
) {
    if !noise_config.is_changed() || noise_config.is_added() {
        return;
    }

    if let Ok((entity, slider)) = offset_x.single() {
        if slider.0 != noise_config.offset.x {
            commands.entity(entity).insert(SliderValue(noise_config.offset.x));
        }
    }
    if let Ok((entity, slider)) = offset_y.single() {
        if slider.0 != noise_config.offset.y {
            commands.entity(entity).insert(SliderValue(noise_config.offset.y));
        }
    }
    if let Ok((entity, slider)) = octaves.single() {
        let target = noise_config.octaves as f32;
        if slider.0 != target {
            commands.entity(entity).insert(SliderValue(target));
        }
    }
    if let Ok((entity, slider)) = seed.single() {
        let target = noise_config.seed.unwrap_or(0) as f32;
        if slider.0 != target {
            commands.entity(entity).insert(SliderValue(target));
        }
    }
    if let Ok((entity, slider)) = noise_scale.single() {
        if slider.0 != noise_config.noise_scale {
            commands.entity(entity).insert(SliderValue(noise_config.noise_scale));
        }
    }
    if let Ok((entity, slider)) = lacunarity.single() {
        if slider.0 != noise_config.lacunarity {
            commands.entity(entity).insert(SliderValue(noise_config.lacunarity));
        }
    }
    if let Ok((entity, slider)) = persistence.single() {
        if slider.0 != noise_config.persistence {
            commands.entity(entity).insert(SliderValue(noise_config.persistence));
        }
    }
    if let Ok((entity, slider)) = height_scale.single() {
        if slider.0 != noise_config.height_scale {
            commands.entity(entity).insert(SliderValue(noise_config.height_scale));
        }
    }
}

// Update label text when slider values change
fn update_slider_labels(
    noise_config: Res<NoiseSettings>,
    mut offset_x: Query<&mut Text, With<OffsetXLabel>>,
    mut offset_y: Query<&mut Text, (With<OffsetYLabel>, Without<OffsetXLabel>)>,
    mut octaves: Query<&mut Text, (With<OctavesLabel>, Without<OffsetXLabel>, Without<OffsetYLabel>)>,
    mut seed: Query<
        &mut Text,
        (
            With<SeedLabel>,
            Without<OffsetXLabel>,
            Without<OffsetYLabel>,
            Without<OctavesLabel>,
        ),
    >,
    mut noise_scale: Query<
        &mut Text,
        (
            With<NoiseScaleLabel>,
            Without<OffsetXLabel>,
            Without<OffsetYLabel>,
            Without<OctavesLabel>,
            Without<SeedLabel>,
        ),
    >,
    mut lacunarity: Query<
        &mut Text,
        (
            With<LacunarityLabel>,
            Without<OffsetXLabel>,
            Without<OffsetYLabel>,
            Without<OctavesLabel>,
            Without<SeedLabel>,
            Without<NoiseScaleLabel>,
        ),
    >,
    mut persistence: Query<
        &mut Text,
        (
            With<PersistenceLabel>,
            Without<OffsetXLabel>,
            Without<OffsetYLabel>,
            Without<OctavesLabel>,
            Without<SeedLabel>,
            Without<NoiseScaleLabel>,
            Without<LacunarityLabel>,
        ),
    >,
    mut height_scale: Query<
        &mut Text,
        (
            With<HeightScaleLabel>,
            Without<OffsetXLabel>,
            Without<OffsetYLabel>,
            Without<OctavesLabel>,
            Without<SeedLabel>,
            Without<NoiseScaleLabel>,
            Without<LacunarityLabel>,
            Without<PersistenceLabel>,
        ),
    >,
) {
    if !noise_config.is_changed() {
        return;
    }

    if let Ok(mut text) = offset_x.single_mut() {
        **text = format!("Offset X: {:.3}", noise_config.offset.x);
    }
    if let Ok(mut text) = offset_y.single_mut() {
        **text = format!("Offset Y: {:.3}", noise_config.offset.y);
    }
    if let Ok(mut text) = octaves.single_mut() {
        **text = format!("Octaves: {}", noise_config.octaves);
    }
    if let Ok(mut text) = seed.single_mut() {
        **text = format!("Seed: {}", noise_config.seed.unwrap_or(0));
    }
    if let Ok(mut text) = noise_scale.single_mut() {
        **text = format!("Noise Scale: {:.4}", noise_config.noise_scale);
    }
    if let Ok(mut text) = lacunarity.single_mut() {
        **text = format!("Lacunarity: {:.3}", noise_config.lacunarity);
    }
    if let Ok(mut text) = persistence.single_mut() {
        **text = format!("Persistence: {:.3}", noise_config.persistence);
    }
    if let Ok(mut text) = height_scale.single_mut() {
        **text = format!("Height Scale: {:.3}", noise_config.height_scale);
    }
}

// Recalculate terrain when NoiseConfig changes
fn recalculate_on_noise_change(
    noise_config: Res<NoiseSettings>,
    chunk_config: Res<ChunkSettings>,
    query: Query<(&TerrainTileId, &mut TerrainMeshlet)>,
) {
    if noise_config.is_changed() && !noise_config.is_added() {
        println!("🔄 Recalculating terrain with new parameters...");
        recalculate_terrain(query, noise_config, chunk_config);
    }
}

