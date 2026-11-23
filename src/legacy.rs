//! Pre-1.13 numeric block IDs (MCEdit / Schematica .schematic) -> modern block states.

use crate::formats::BlockState;

pub const COLORS: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple",
    "blue", "brown", "green", "red", "black",
];
const WOODS: [&str; 6] = ["oak", "spruce", "birch", "jungle", "acacia", "dark_oak"];
const DIR6: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
/// BlockRailBase.EnumRailDirection metadata order.
const RAIL: [&str; 10] = [
    "north_south", "east_west", "ascending_east", "ascending_west", "ascending_north", "ascending_south", "south_east", "south_west",
    "north_west", "north_east",
];
/// EnumFacing.getHorizontal order.
const HORIZ: [&str; 4] = ["south", "west", "north", "east"];

#[rustfmt::skip]
const NAMES: [&str; 256] = [
    "air","stone","grass_block","dirt","cobblestone","oak_planks","oak_sapling","bedrock","water","water",
    "lava","lava","sand","gravel","gold_ore","iron_ore","coal_ore","oak_log","oak_leaves","sponge",
    "glass","lapis_ore","lapis_block","dispenser","sandstone","note_block","red_bed","powered_rail","detector_rail","sticky_piston",
    "cobweb","short_grass","dead_bush","piston","piston_head","white_wool","moving_piston","dandelion","poppy","brown_mushroom",
    "red_mushroom","gold_block","iron_block","smooth_stone","smooth_stone_slab","bricks","tnt","bookshelf","mossy_cobblestone","obsidian",
    "torch","fire","spawner","oak_stairs","chest","redstone_wire","diamond_ore","diamond_block","crafting_table","wheat",
    "farmland","furnace","furnace","oak_sign","oak_door","ladder","rail","cobblestone_stairs","oak_wall_sign","lever",
    "stone_pressure_plate","iron_door","oak_pressure_plate","redstone_ore","redstone_ore","redstone_torch","redstone_torch","stone_button","snow","ice",
    "snow_block","cactus","clay","sugar_cane","jukebox","oak_fence","pumpkin","netherrack","soul_sand","glowstone",
    "nether_portal","jack_o_lantern","cake","repeater","repeater","white_stained_glass","oak_trapdoor","infested_stone","stone_bricks","brown_mushroom_block",
    "red_mushroom_block","iron_bars","glass_pane","melon","pumpkin_stem","melon_stem","vine","oak_fence_gate","brick_stairs","stone_brick_stairs",
    "mycelium","lily_pad","nether_bricks","nether_brick_fence","nether_brick_stairs","nether_wart","enchanting_table","brewing_stand","cauldron","end_portal",
    "end_portal_frame","end_stone","dragon_egg","redstone_lamp","redstone_lamp","oak_planks","oak_slab","cocoa","sandstone_stairs","emerald_ore",
    "ender_chest","tripwire_hook","tripwire","emerald_block","spruce_stairs","birch_stairs","jungle_stairs","command_block","beacon","cobblestone_wall",
    "flower_pot","carrots","potatoes","oak_button","skeleton_skull","anvil","trapped_chest","light_weighted_pressure_plate","heavy_weighted_pressure_plate","comparator",
    "comparator","daylight_detector","redstone_block","nether_quartz_ore","hopper","quartz_block","quartz_stairs","activator_rail","dropper","white_terracotta",
    "white_stained_glass_pane","acacia_leaves","acacia_log","acacia_stairs","dark_oak_stairs","slime_block","barrier","iron_trapdoor","prismarine","sea_lantern",
    "hay_block","white_carpet","terracotta","coal_block","packed_ice","sunflower","white_banner","white_wall_banner","daylight_detector","red_sandstone",
    "red_sandstone_stairs","red_sandstone","red_sandstone_slab","spruce_fence_gate","birch_fence_gate","jungle_fence_gate","dark_oak_fence_gate","acacia_fence_gate","spruce_fence","birch_fence",
    "jungle_fence","dark_oak_fence","acacia_fence","spruce_door","birch_door","jungle_door","acacia_door","dark_oak_door","end_rod","chorus_plant",
    "chorus_flower","purpur_block","purpur_pillar","purpur_stairs","purpur_block","purpur_slab","end_stone_bricks","beetroots","dirt_path","end_gateway",
    "repeating_command_block","chain_command_block","frosted_ice","magma_block","nether_wart_block","red_nether_bricks","bone_block","structure_void","observer","white_shulker_box",
    "orange_shulker_box","magenta_shulker_box","light_blue_shulker_box","yellow_shulker_box","lime_shulker_box","pink_shulker_box","gray_shulker_box","light_gray_shulker_box","cyan_shulker_box","purple_shulker_box",
    "blue_shulker_box","brown_shulker_box","green_shulker_box","red_shulker_box","black_shulker_box","white_glazed_terracotta","orange_glazed_terracotta","magenta_glazed_terracotta","light_blue_glazed_terracotta","yellow_glazed_terracotta",
    "lime_glazed_terracotta","pink_glazed_terracotta","gray_glazed_terracotta","light_gray_glazed_terracotta","cyan_glazed_terracotta","purple_glazed_terracotta","blue_glazed_terracotta","brown_glazed_terracotta","green_glazed_terracotta","red_glazed_terracotta",
    "black_glazed_terracotta","white_concrete","white_concrete_powder","stone","stone","structure_block",
];

fn bs(name: impl Into<String>, props: &[(&str, &str)]) -> BlockState {
    BlockState {
        name: name.into(),
        props: props.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    }
}

fn is_stairs(id: u16) -> bool {
    matches!(id, 53 | 67 | 108 | 109 | 114 | 128 | 134 | 135 | 136 | 156 | 163 | 164 | 180 | 203)
}
pub fn is_door(id: u16) -> bool {
    matches!(id, 64 | 71 | 193..=197)
}

/// `lower_data` is only used for upper door halves (data of the block below).
pub fn legacy_state(id: u16, data: u8, lower_data: u8) -> BlockState {
    let d = data & 15;
    let name = NAMES.get(id as usize).copied().unwrap_or("stone");
    match id {
        1 => bs(
            ["stone", "granite", "polished_granite", "diorite", "polished_diorite", "andesite", "polished_andesite"]
                .get(d as usize)
                .copied()
                .unwrap_or("stone"),
            &[],
        ),
        3 => bs(["dirt", "coarse_dirt", "podzol"].get(d as usize).copied().unwrap_or("dirt"), &[]),
        5 | 125 => bs(format!("{}_planks", WOODS[(d & 7).min(5) as usize]), &[]),
        6 => bs(format!("{}_sapling", WOODS[(d & 7).min(5) as usize]), &[]),
        126 => bs(
            format!("{}_slab", WOODS[(d & 7).min(5) as usize]),
            &[("type", if d & 8 != 0 { "top" } else { "bottom" })],
        ),
        12 => bs(if d == 1 { "red_sand" } else { "sand" }, &[]),
        17 | 162 => {
            let w = if id == 17 { WOODS[(d & 3) as usize] } else { WOODS[4 + (d & 1) as usize] };
            let axis = ["y", "x", "z", "y"][(d >> 2) as usize];
            if d >> 2 == 3 {
                bs(format!("{w}_wood"), &[])
            } else {
                bs(format!("{w}_log"), &[("axis", axis)])
            }
        }
        18 | 161 => {
            let w = if id == 18 { WOODS[(d & 3) as usize] } else { WOODS[4 + (d & 1) as usize] };
            bs(format!("{w}_leaves"), &[])
        }
        31 => bs(["dead_bush", "short_grass", "fern", "fern"][(d & 3) as usize], &[]),
        38 => bs(
            ["poppy", "blue_orchid", "allium", "azure_bluet", "red_tulip", "orange_tulip", "white_tulip", "pink_tulip", "oxeye_daisy"]
                .get(d as usize)
                .copied()
                .unwrap_or("poppy"),
            &[],
        ),
        35 => bs(format!("{}_wool", COLORS[d as usize]), &[]),
        95 => bs(format!("{}_stained_glass", COLORS[d as usize]), &[]),
        159 => bs(format!("{}_terracotta", COLORS[d as usize]), &[]),
        160 => bs(format!("{}_stained_glass_pane", COLORS[d as usize]), &[]),
        171 => bs(format!("{}_carpet", COLORS[d as usize]), &[]),
        251 => bs(format!("{}_concrete", COLORS[d as usize]), &[]),
        252 => bs(format!("{}_concrete_powder", COLORS[d as usize]), &[]),
        43 => bs(
            ["smooth_stone", "sandstone", "oak_planks", "cobblestone", "bricks", "stone_bricks", "nether_bricks", "quartz_block"]
                [(d & 7) as usize],
            &[],
        ),
        44 => bs(
            [
                "smooth_stone_slab", "sandstone_slab", "oak_slab", "cobblestone_slab", "brick_slab", "stone_brick_slab",
                "nether_brick_slab", "quartz_slab",
            ][(d & 7) as usize],
            &[("type", if d & 8 != 0 { "top" } else { "bottom" })],
        ),
        182 | 205 => bs(name, &[("type", if d & 8 != 0 { "top" } else { "bottom" })]),
        98 => bs(
            ["stone_bricks", "mossy_stone_bricks", "cracked_stone_bricks", "chiseled_stone_bricks"][(d & 3) as usize],
            &[],
        ),
        168 => bs(["prismarine", "prismarine_bricks", "dark_prismarine", "prismarine"][(d & 3) as usize], &[]),
        139 => bs(if d == 1 { "mossy_cobblestone_wall" } else { "cobblestone_wall" }, &[]),
        175 => bs(
            ["sunflower", "lilac", "tall_grass", "large_fern", "rose_bush", "peony", "sunflower", "sunflower"][(d & 7) as usize],
            &[],
        ),
        29 | 33 => bs(
            name,
            &[("facing", DIR6[(d & 7).min(5) as usize]), ("extended", if d & 8 != 0 { "true" } else { "false" })],
        ),
        34 => bs(
            name,
            &[("facing", DIR6[(d & 7).min(5) as usize]), ("type", if d & 8 != 0 { "sticky" } else { "normal" })],
        ),
        23 | 158 | 218 | 154 | 198 => bs(name, &[("facing", DIR6[(d & 7).min(5) as usize])]),
        50 | 75 | 76 => {
            let wall = match d {
                1 => Some("east"),
                2 => Some("west"),
                3 => Some("south"),
                4 => Some("north"),
                _ => None,
            };
            let lit = if id == 75 { "false" } else { "true" };
            let base = if id == 50 { "torch" } else { "redstone_torch" };
            match wall {
                Some(f) => bs(
                    if id == 50 { "wall_torch".to_string() } else { "redstone_wall_torch".to_string() },
                    &[("facing", f), ("lit", lit)],
                ),
                None => bs(base, &[("lit", lit)]),
            }
        }
        69 => {
            let (face, facing) = match d & 7 {
                0 | 7 => ("ceiling", "south"),
                1 => ("wall", "east"),
                2 => ("wall", "west"),
                3 => ("wall", "south"),
                4 => ("wall", "north"),
                _ => ("floor", "south"),
            };
            bs(name, &[("face", face), ("facing", facing), ("powered", if d & 8 != 0 { "true" } else { "false" })])
        }
        77 | 143 => {
            let (face, facing) = match d & 7 {
                0 => ("ceiling", "south"),
                1 => ("wall", "east"),
                2 => ("wall", "west"),
                3 => ("wall", "south"),
                4 => ("wall", "north"),
                _ => ("floor", "south"),
            };
            bs(name, &[("face", face), ("facing", facing)])
        }
        55 => {
            let p = d.to_string();
            bs(
                name,
                &[("power", &p), ("north", "side"), ("south", "side"), ("east", "side"), ("west", "side")],
            )
        }
        93 | 94 => {
            let delay = ((d >> 2) + 1).to_string();
            bs(
                name,
                &[("facing", HORIZ[(d & 3) as usize]), ("delay", &delay), ("powered", if id == 94 { "true" } else { "false" })],
            )
        }
        149 | 150 => bs(
            name,
            &[
                ("facing", HORIZ[(d & 3) as usize]),
                ("mode", if d & 4 != 0 { "subtract" } else { "compare" }),
                ("powered", if d & 8 != 0 || id == 150 { "true" } else { "false" }),
            ],
        ),
        64 | 71 | 193..=197 => {
            let (low, up) = if d & 8 != 0 { (lower_data, d) } else { (d, 0) };
            let facing = ["east", "south", "west", "north"][(low & 3) as usize];
            bs(
                name,
                &[
                    ("facing", facing),
                    ("open", if low & 4 != 0 { "true" } else { "false" }),
                    ("half", if d & 8 != 0 { "upper" } else { "lower" }),
                    ("hinge", if up & 1 != 0 { "right" } else { "left" }),
                ],
            )
        }
        96 | 167 => bs(
            name,
            &[
                ("facing", ["north", "south", "west", "east"][(d & 3) as usize]),
                ("open", if d & 4 != 0 { "true" } else { "false" }),
                ("half", if d & 8 != 0 { "top" } else { "bottom" }),
            ],
        ),
        107 | 183..=187 => bs(
            name,
            &[("facing", HORIZ[(d & 3) as usize]), ("open", if d & 4 != 0 { "true" } else { "false" })],
        ),
        65 | 68 | 177 | 54 | 130 | 146 | 61 | 62 => {
            let f = match d {
                3 => "south",
                4 => "west",
                5 => "east",
                _ => "north",
            };
            bs(name, &[("facing", f)])
        }
        78 => {
            let l = ((d & 7) + 1).to_string();
            bs(name, &[("layers", &l)])
        }
        26 => bs(
            name,
            &[("facing", HORIZ[(d & 3) as usize]), ("part", if d & 8 != 0 { "head" } else { "foot" })],
        ),
        155 => match d {
            1 => bs("chiseled_quartz_block", &[]),
            2..=4 => bs("quartz_pillar", &[("axis", ["y", "x", "z"][(d - 2) as usize])]),
            _ => bs("quartz_block", &[]),
        },
        170 | 202 | 216 => bs(name, &[("axis", ["y", "x", "z", "y"][((d >> 2) & 3) as usize])]),
        66 => bs(name, &[("shape", RAIL.get(d as usize).copied().unwrap_or("north_south"))]),
        27 | 28 | 157 => bs(
            name,
            &[("shape", RAIL.get((d & 7) as usize).copied().unwrap_or("north_south")), ("powered", if d & 8 != 0 { "true" } else { "false" })],
        ),
        145 => bs(["anvil", "chipped_anvil", "damaged_anvil", "anvil"][((d >> 2) & 3) as usize], &[("facing", HORIZ[(d & 3) as usize])]),
        86 => bs("carved_pumpkin", &[("facing", HORIZ[(d & 3) as usize])]),
        91 => bs(name, &[("facing", HORIZ[(d & 3) as usize])]),
        120 => bs(name, &[("facing", HORIZ[(d & 3) as usize]), ("eye", if d & 4 != 0 { "true" } else { "false" })]),
        8 | 9 => bs("water", &[("level", &d.to_string())]),
        10 | 11 => bs("lava", &[("level", &d.to_string())]),
        _ if is_stairs(id) => bs(
            name,
            &[
                ("facing", ["east", "west", "south", "north"][(d & 3) as usize]),
                ("half", if d & 4 != 0 { "top" } else { "bottom" }),
                ("shape", "straight"),
            ],
        ),
        _ => bs(name, &[]),
    }
}
