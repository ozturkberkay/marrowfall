# MARROWFALL

## Game Concept Document

---

## Elevator Pitch

**Marrowfall** is a single-player isometric action RPG sandbox set in a dying
medieval world overrun by the undead, demons, and worse. There are no classes,
no quest markers, and no loot piñatas. Everything you wield, you crafted. Every
skill you have, you earned by doing. The world is bleak, lonely, and hostile —
and it doesn't care if you survive. You kill a skeleton, you take its bones, you
forge a shield from them. You study a forbidden scroll, you gather rare
reagents, you cast a spell that levels a crypt. You decide what kind of survivor
you become. There is no story. There is only Marrowfall.

**Genre:** Single-player isometric action RPG sandbox
**Perspective:** Isometric (Diablo 2 / Ultima Online style)
**Platform:** PC (primary)
**Inspirations:** Diablo 2, Ultima Online, Path of Exile 2, Dark Souls

---

## Design Pillars

These are the non-negotiable principles that guide every design decision in
Marrowfall.

### 1. Earned, Not Given

Nothing drops ready-made. Enemies yield raw materials — bones, hides, ichor,
scales, ore fragments embedded in their flesh. The player transforms these
materials into weapons, armor, tools, and more through crafting professions.
Every piece of gear has a history: you killed the thing, you harvested it, you
made the item. This creates a relationship between the player and their
equipment that loot-drop games can never achieve.

### 2. You Are What You Do

There are no classes. No character creation screen where you pick "Warrior" or
"Mage." Your character is defined entirely by what you choose to spend your time
doing. Swing a sword and your swordsmanship improves. Mine ore and your mining
improves. Study scrolls and your arcane knowledge grows. This is the Ultima
Online model: organic, classless, use-based progression. The player is never
locked into a path and can pivot at any time — but mastery takes serious
investment.

### 3. Alone in the Dark

The world of Marrowfall is bleak, lonely, and oppressive. Humans are rare, and
where you find them is a camp or a passing caravan rather than a city. Between
those: nothing but ruins, crypts, corrupted wilderness, and the things that live
in them. The player spends most of their time alone. There are no companions, no
party members, no friendly faces around the next corner. This isolation is a
core feature, not a limitation. Because nowhere is on a map you were handed, the
rare moments of safety are ones you found: cresting a rise and seeing a campfire
should feel like genuine relief.

### 4. Death Has Teeth

Combat is punishing. Enemies hit hard, and the player can die quickly if
careless. This is not a power-fantasy where you mow through hundreds of mobs —
every encounter demands attention.

- **Normal mode:** Death carries a meaningful penalty — gear durability loss,
  dropped materials, a corpse run to recover belongings.
- **Hardcore mode:** Permadeath. One life. When you die, that character is gone
  forever. This mode is where Marrowfall becomes its most tense and rewarding.

### 5. Magic Costs Something

Magic exists in Marrowfall, and when mastered, it is devastatingly powerful. But
that power is earned through serious investment: finding rare tomes and scrolls,
studying them, gathering specific material components, and paying a real cost to
cast. Magic is not a casual tool — it is a fearsome discipline that demands
dedication. A master mage in Marrowfall should feel like a force of nature,
precisely because it took so much to get there.

---

## World & Setting

### The World

Marrowfall takes place in a dark medieval fantasy world that has already ended.
A great civilization once stood here — its castles, cities, mines, and temples
are now ruins. What destroyed it is never explicitly stated. There are fragments
— inscriptions on crumbling walls, muttered words from old NPCs, symbols etched
into dungeon floors — but no narrator, no lore dump, no quest that explains it
all. The world is simply *this way now*, and the player exists within it.

The tone is **bleaker than Diablo 2**. Where Diablo had campsites and allies and
a sense of fighting *back* against evil, Marrowfall offers no such comfort. The
evil has already won. What remains is scavengers, survivors, and the things that
hunt them.

### No Story, No Main Quest

Marrowfall is a **pure sandbox**. There is no main quest line, no final boss, no
narrative arc. The player is dropped into the world and must decide for
themselves what to do. Some possible player-driven goals:

- Master every crafting profession
- Clear the deepest, most dangerous dungeons
- Amass wealth and rare materials
- Become a feared mage wielding devastating spells
- Survive as long as possible in hardcore mode
- Explore every corner of the generated world
- Build the most powerful gear possible from the rarest materials

NPCs may offer **bounties or requests** (kill this creature, bring me this
material), but these are optional tasks, not a story. They exist to give
direction to players who want it, without forcing a narrative on those who
don't.

### World Structure

**The whole world comes from a seed, and it has no edge.** Nothing is
hand-placed. Every biome, every hill, every camp and every ruin is decided by
the seed, so two players on different seeds share the rules of the world but
none of its geography.

**Danger grows with distance from where you woke up.** That is the one rule the
world guarantees, and it is what turns walking into progression:

- The land around the origin is the safest ground in the world. It is where a
  new survivor learns to fight and where a Hardcore character retreats to.
- Every few kilometres outward, the land steps up a difficulty band. The bands
  are not visible as lines; they show as the biomes changing character.
- The frontier, is the hardest band and it runs on forever. There is no wall
  and no last zone.

**Places are found, not authored.** The world holds points of interest, and they
are scattered by spacing rules rather than placed by hand: a rule says how often
a kind of place appears, how far apart two of them must be, how far from the
origin it may start, and which difficulty bands it belongs to. So the density of
places is designed while every location is yours to discover.

### Biomes (Examples)

A biome is a kind of land, not a place. The same biome appears wherever the seed
puts it, as many times as the seed puts it, and it belongs to one difficulty
band. These are examples of the kind, not a list of the world's contents.

| Band | Biome | Description | Key Resources | Threats |
|------|-------|-------------|---------------|---------|
| Safest | **Ashen lowland** | Fog-choked flatlands of dead grass and crumbling farmsteads | Iron ore, leather, bone | Skeletons, ghouls, wild dogs |
| Near | **Blackweald** | Dense, lightless forest where the trees have petrified | Dark wood, spider silk, venom sacs | Giant spiders, corrupted treants, wraiths |
| Middle | **Scoured rock** | Bare stone terraces stripped down to the bone of the land | Silver, gemstones, sulfur | Cave trolls, bats, mine spirits |
| Far | **Cinder waste** | Ground that still smoulders, littered with fortress rubble | Steel fragments, runic stones, ash powder | Demons, fire elementals, cursed knights |
| Far | **Barrow field** | Low mounds over graves nobody remembers digging | Grave dust, enchanted bones, sealed tomes | Vampires, liches, bone constructs |
| Frontier | **Emberpeak** | Volcanic high ground, home to the most dangerous creatures | Dragon scale, obsidian, magma ore | Dragons, wyverns, flame drakes |

---

## Core Gameplay Loop

The fundamental cycle of Marrowfall:

```
EXPLORE  -->  FIGHT  -->  HARVEST  -->  CRAFT  -->  GROW  -->  PUSH DEEPER
  |                                                               |
  +---------------------------------------------------------------+
```

1. **Explore** — Venture out from safety into the unknown. Discover new biomes,
   caves, camps, resource nodes.
2. **Fight** — Engage enemies with punishing, deliberate combat. Every encounter
   is dangerous.
3. **Harvest** — Collect materials from slain enemies (bones, hides, scales,
   ichor) and from the environment (ore veins, herbs, wood).
4. **Craft** — Return to a workstation. Use gathered materials + your profession
   skills to create gear, tools, consumables, and components.
5. **Grow** — Your skills improve through use. Your gear improves through better
   crafting. You become more capable.
6. **Push Further** — Walk further out than you safely could before. Find rarer
   materials. Face stronger enemies. The cycle continues.

There is no endpoint. The player decides when they're done — or in hardcore
mode, the world decides for them.

---

## Combat

### Philosophy

Combat in Marrowfall is inspired by **Path of Exile 2's early beta** — the
period when players called it "the Dark Souls of ARPGs." It is **punishing,
deliberate, and dangerous**. The player cannot mindlessly click through packs of
monsters. Mobs hit hard, and the player must pay attention to positioning,
timing, and resource management.

### Controls

- **WASD movement** (not click-to-move). This gives the player direct,
  responsive control over their character and enables the kind of precise
  positioning that punishing combat demands.
- **Mouse** for aiming attacks, targeting, interacting with the world.
- **Dodge roll** on a cooldown or stamina cost. A quick evasive maneuver to
  avoid incoming attacks. Timing matters — rolling too early or too late gets
  you hit.
- **Jumping** — a novel mechanic for the isometric ARPG genre. Jump over low
  obstacles, leap across gaps, drop down ledges. Opens up vertical gameplay in
  an isometric space: climb ruins, jump to reach platforms, escape enemies by
  taking unexpected paths. Creates opportunities for environmental exploration
  that no other ARPG offers.

### Combat Mechanics

- **Stamina system:** Attacks, dodges, and jumps consume stamina. Stamina
  regenerates over time but forces the player to manage aggression vs. defense.
  Going all-in on offense leaves you unable to dodge. Playing too defensively
  wastes openings.
- **Weapon types determine fighting style:** A character using a greatsword
  fights fundamentally differently from one using dual daggers or a mace and
  shield. This isn't about class restrictions — any character can pick up any
  weapon — but each weapon type has its own moveset, speed, reach, and stamina
  costs.
- **No auto-targeting or aim assist.** The player must position and aim. Attacks
  can miss. This rewards skillful play and makes every hit feel earned.
- **Enemy telegraphs:** Dangerous attacks are telegraphed through animations,
  giving the player a window to react. Reading enemy patterns is key to
  survival, especially against tougher foes.
- **Environmental hazards:** Crumbling floors, fire traps, poison pools,
  collapsing structures. The dungeon itself is an enemy.

### Weapon Categories (Examples)

| Category | Speed | Reach | Style |
|----------|-------|-------|-------|
| **Swords (1H)** | Medium | Medium | Balanced, versatile |
| **Greatswords** | Slow | Long | Heavy hits, wide swings, stamina-hungry |
| **Daggers (Dual)** | Fast | Short | Quick strikes, high mobility, low damage per hit |
| **Maces/Hammers** | Slow | Short | Devastating single-target, armor-crushing |
| **Spears/Polearms** | Medium | Long | Keep distance, thrust-focused |
| **Bows** | Varies | Ranged | Kiting, precision shots, ammo-dependent |
| **Shields** | N/A | N/A | Block incoming damage, shield bash for stagger |

---

## Skill & Progression System

### No Classes

There are no classes in Marrowfall. The player character starts as a blank slate
— a survivor with no inherent specialization. What the player becomes is
determined entirely by what they choose to do.

### Skill-by-Use

Skills improve through practice, not by spending points on a tree. This is the
**Ultima Online model**:

- Swing a sword 100 times → Swordsmanship increases
- Mine ore for an hour → Mining increases
- Successfully forge a piece of armor → Blacksmithing increases
- Dodge an enemy attack → Evasion increases
- Take a hit while blocking → Shield Defense increases
- Cast a fire spell → Fire Magic increases

Every meaningful action in the game is tied to a skill, and performing that
action is what makes you better at it.

### Skill Progression

- Skills are measured on a scale (e.g., 0-100 or a tiered system: Novice →
  Apprentice → Journeyman → Expert → Master → Grandmaster).
- **Lower levels** increase quickly to reward initial exploration of a new
  skill.
- **Higher levels** require significant sustained effort, creating a sense of
  long-term mastery.
- **Diminishing returns** prevent rapid power-spiking. Going from 90 to 100
  takes far longer than 0 to 50.
- There may be a **soft cap** on total skill points across all skills, forcing
  the player to make meaningful choices about what to prioritize (or accept
  being a generalist who masters nothing). This is a key balance decision.

### Skill Categories

#### Combat Skills

- Swordsmanship, Axe Fighting, Mace Fighting, Polearm Fighting
- Archery, Throwing
- Shield Defense, Parrying
- Evasion (dodge effectiveness)
- Unarmed Combat
- Tactics (passive combat damage bonus from experience)

#### Crafting & Gathering Skills (Professions)

- Mining, Lumberjacking, Herbalism, Skinning, Butchering
- Blacksmithing, Leatherworking, Woodworking, Alchemy
- Jewelcrafting, Enchanting, Cooking
- Tinkering (traps, tools, mechanical devices)

#### Magic Skills

- Arcane Knowledge (general magical understanding)
- Fire Magic, Frost Magic, Shadow Magic, Nature Magic
- Inscription (creating scrolls, runes)
- Ritual Magic (powerful but slow, requires preparation and rare components)

#### Utility Skills

- Stealth (move unseen, avoid encounters)
- Lockpicking (open locked chests, doors)
- Cartography (reveal more of the map, mark resources)
- Athletics (affects stamina, jump distance, movement speed)
- Perception (spot hidden traps, secret passages, rare resources)

---

## Crafting & Professions

### The Core System

Crafting is **the** defining system of Marrowfall. In most ARPGs, you kill a
monster and it drops a sword. In Marrowfall, you kill a monster and it drops
*what that monster is made of*. You then take those materials to a workstation
and make the sword yourself.

This means:
- The player understands where every item came from
- Gear has personal significance (you remember the fight that yielded the
  materials)
- Progression is tied to player knowledge and profession skill, not random loot
  tables
- The economy is driven by materials, not item drops

### Material Harvesting

Enemies drop materials based on what they are:

| Enemy | Drops |
|-------|-------|
| **Skeleton** | Bone fragments, bone dust, rusted metal scraps |
| **Zombie** | Rotting leather, sinew, grave dust |
| **Giant Spider** | Spider silk, venom sacs, chitin plates |
| **Demon** | Demon hide, brimstone, infernal essence |
| **Vampire** | Dark ichor, enchanted cloth, cursed fangs |
| **Dragon** | Dragon scales, dragon bone, magma ore, dragon blood |
| **Troll** | Troll hide (tough), troll fat (alchemy), massive bones |
| **Wraith** | Ectoplasm, spirit essence, shadow thread |

The world also provides materials through gathering:
- **Mining:** Iron, silver, gold, gemstones, obsidian, sulfur
- **Lumberjacking:** Various woods (oak, dark pine, petrified wood,
  spirit-touched yew)
- **Herbalism:** Healing herbs, poisonous plants, mushrooms, rare reagents
- **Skinning/Butchering:** Hides, meat, fat, sinew from beasts

### Workstations

Crafting requires the right workstation. Some are found standing at camps and
ruins; the rest you build at a camp you have claimed:

- **Forge & Anvil** — Metal weapons and armor (Blacksmithing)
- **Tanning Rack** — Leather processing and leather armor (Leatherworking)
- **Woodworking Bench** — Bows, shields, hafts, wooden items (Woodworking)
- **Alchemy Table** — Potions, poisons, reagent processing (Alchemy)
- **Jeweler's Tools** — Rings, amulets, gem cutting (Jewelcrafting)
- **Enchanting Circle** — Applying magical properties to gear (Enchanting)
- **Cooking Fire** — Food that provides temporary buffs (Cooking)
- **Inscriber's Desk** — Scrolls, runes, magical writings (Inscription)

### Recipe Discovery

Recipes are not all known from the start. They are discovered through:

- **Experimentation:** Combine materials at a workstation and see what you can
  make. Higher skill reveals more possibilities.
- **Found recipes:** Discover recipe scrolls in dungeons, treasure chests, or
  hidden locations.
- **NPC knowledge:** Certain NPCs teach recipes in exchange for materials,
  favors, or gold.
- **Skill milestones:** Reaching certain skill levels unlocks knowledge of new
  recipes automatically (a master blacksmith intuitively knows advanced
  techniques).

### Crafting Quality

The quality of crafted items depends on:

1. **Profession skill level** — A novice blacksmith makes crude iron swords. A
   grandmaster makes exceptional ones.
2. **Material quality** — Dragon bone produces better results than common bone.
   Rare materials enable unique items.
3. **Tools** — Better crafting tools improve outcomes (crafted tools — another
   loop).
4. **Critical success** — Small random chance of an exceptional result, weighted
   by skill level.

Quality tiers (example): Crude → Common → Fine → Superior → Exceptional →
Masterwork

### Example Crafting Chains

**Bone Shield:**
Kill skeletons → Collect bone fragments (8) + sinew (3) → Workbench → Bone
Shield (requires Woodworking 20+)

**Dragonscale Armor:**
Kill dragon → Collect dragon scales (15) + dragon bone (5) → Collect steel
ingots (10, from mining + smelting) → Forge → Dragonscale Plate (requires
Blacksmithing 85+)

**Venom-Tipped Arrows:**
Kill giant spiders → Collect venom sacs (5) → Collect wood shafts (from
Woodworking) + iron arrowheads (from Blacksmithing) → Alchemy Table →
Venom-Tipped Arrows (requires Alchemy 30+, Woodworking 15+)

---

## Magic System

### Philosophy

Magic in Marrowfall is **powerful, costly, and feared**. It is not a casual
tool. A fireball isn't something you spam — it's something you prepared for,
gathered components for, and unleashed at the right moment to devastating
effect. A master mage in Marrowfall should feel like a walking catastrophe,
precisely because of the investment required to become one.

### Learning Magic

Magic is not innate. It must be **studied and learned**:

1. **Find a source of knowledge** — Tomes, scrolls, and grimoires hidden in
   dungeons, crypts, and ancient libraries. Some NPCs in remote locations may
   teach basic spells.
2. **Study it** — Reading a tome isn't instant. The player must spend time
   studying, and their Arcane Knowledge skill determines how quickly they
   comprehend new spells. A scroll that takes a novice days of study might take
   a master an hour.
3. **Practice** — Newly learned spells are weak and unreliable. Only through
   repeated casting does proficiency (and the associated magic skill) increase.

### Casting Costs

Spells require **material components** — gathered or crafted reagents consumed
on casting:

| Spell Example | Components Required |
|---------------|-------------------|
| **Fire Bolt** | Sulfur (1), Brimstone Dust (1) |
| **Frost Shield** | Frost Crystal (2), Spirit Essence (1) |
| **Shadow Step** | Shadow Thread (1), Ectoplasm (1) |
| **Raise Dead** | Grave Dust (5), Bone Fragment (3), Dark Ichor (1) |
| **Infernal Storm** | Brimstone (10), Dragon Blood (3), Infernal Essence (5) |

This means:
- Casting isn't free — you must gather and carry components
- Powerful spells require rare, dangerous-to-acquire materials
- The player must decide when to use their precious reagents vs. saving them
- A mage character is deeply tied to the gathering and crafting loop (harvesting
  spell components)

### Magic Disciplines

- **Fire Magic** — Destruction, burning, area denial. Powerful offense, poor
  subtlety.
- **Frost Magic** — Slowing, freezing, defensive barriers. Control-focused.
- **Shadow Magic** — Stealth, teleportation, fear, debuffs. Utility and evasion.
- **Nature Magic** — Healing, poison, entangling, beast-related effects.
  Survival-focused.
- **Ritual Magic** — The most powerful and costly school. Requires preparation
  (drawing circles, placing components, chanting). Cannot be cast in combat.
  Used for enchanting, summoning, warding, and world-altering effects. A ritual
  to ward an area against undead. A ritual to open a sealed door. A ritual to
  commune with the dead for knowledge.

### The Cost of Power

Magic at high levels may carry **consequences beyond material costs**:

- Physical toll (temporary stat reduction after casting powerful spells)
- Environmental reaction (the world responds to magic use — creatures drawn to
  magical energy)
- Corruption risk (overuse of certain schools, particularly Shadow and Ritual,
  may have cumulative effects)

These consequences reinforce the design pillar: magic is powerful, but it costs
something real.

---

## Economy & NPCs

### NPC Role

NPCs in Marrowfall are **rare and purposeful**. Seeing another human face should
feel notable.

- **Merchants** — Buy and sell materials, basic gear, recipes, and crafting
  tools. Each merchant has limited, rotating stock. They are not infinite
  vending machines.
- **Lore sources** — Old survivors, hermits, and scholars who provide fragments
  of world history through dialogue. No cutscenes, no forced exposition — the
  player chooses to engage or not.
- **Task-givers** — NPCs may offer optional bounties ("Kill the troll in the
  western caves") or requests ("Bring me 10 iron ingots"). These provide
  direction and rewards (gold, rare recipes, unique materials) but are never
  mandatory. There is no quest log. No main quest.
- **Specialist trainers** — Rare NPCs who can teach specific skills or spells
  that are difficult or impossible to learn through other means.

### Economy

- **Currency:** Gold exists but is scarce. The economy is tight — not a power
  fantasy where the player hoards millions.
- **Trading:** Barter with NPCs. Some may prefer materials over gold. Prices
  vary from one camp or caravan to the next.
- **No auction house, no player economy** — this is single-player. The economy
  is between you and the NPCs.
- **Material scarcity:** Rare materials are genuinely rare. Dragon scales don't
  grow on trees. This scarcity gives high-tier crafting real weight.

---

## Death & Difficulty

### Philosophy

Marrowfall does not have a difficulty slider. The world is consistently
dangerous. The challenge comes from the world itself, not from an artificial
setting.

### Normal Mode

- On death, the player respawns at the last safe point they claimed, or at the
  world origin if they have claimed none. There is nowhere on the map that is
  safe by default, so a safe point is something you make.
- **Corpse run:** The player's body remains where they died, along with any
  carried materials and unequipped gear. The player must return to their corpse
  to recover their belongings. If they die again before reaching it, the first
  corpse's contents are lost.
- **Gear durability loss:** Equipped items lose durability on death. Repairing
  them costs materials. Repeated deaths grind your gear down.
- **No skill loss:** Skills are never lost on death. What you learned, you keep.

### Hardcore Mode

- **Permadeath.** One life. When you die, that character is gone.
- Same world, same rules, infinitely higher stakes.
- Every encounter, every dungeon, every decision carries the weight of finality.
- This is the ultimate expression of Marrowfall's design: when everything you've
  crafted and earned can be lost forever, every moment matters.

### Enemy Scaling

Enemies do **not** scale to the player's level (there is no player level).
Difficulty comes from **where you are standing**, and specifically from how far
that is from the origin. A patch of land near the origin is always survivable
for a new character. The frontier is always lethal to all but the most prepared.

Because the rule is distance and not the player, nothing you do makes the world
harder. Walking further does. The player learns how far out they can currently
go, and pushing that line is the progression.

---

## World Generation & Replayability

### What Every World Shares

The rules, never the map:

- **Danger grows outward.** The origin is the safest ground and the frontier is
  the hardest, on every seed.
- **Biome identity.** A blackweald is always a dark petrified forest, wherever
  it turns up, and it always belongs to the same difficulty band.
- **Spacing rules.** Each kind of place has a density and a minimum gap, so some
  are common and close together while others are rare and far apart. How often a
  kind appears is authored; where it appears is not.
- **The origin is where you wake up**, and it is what every distance is measured
  from.

### What Changes With The Seed

- **The whole map.** Which biomes exist, how big each patch is, which direction
  each one lies in, and where the land rises and falls.
- **Every place in it.** Where each point of interest sits, and which kind it is.
- **Underground layouts.** If a place leads underground, the interior is its own
  generated space, so it is never the same twice across playthroughs.
- **Resource placement.** Ore veins, herb patches and other gathering nodes
  follow the biomes, so they move when the biomes do.
- **Enemy encounters.** Biome-appropriate enemies are consistent (spiders in a
  blackweald), but their placement and composition varies.

### Replayability Drivers

- A new seed is a genuinely new world, not a reshuffle of known places
- Permadeath hardcore mode creates natural replayability (each death is a fresh
  start)
- The classless skill system encourages different build paths per playthrough
- Rare recipe and material locations change, creating different crafting
  opportunities

---

## Art Direction & Audio

### Visual Tone

- **Palette:** Muted, desaturated. Grays, browns, deep reds, sickly greens.
  Color is rare and meaningful — a glowing rune, a pool of lava, a rare herb —
  should stand out against the decay.
- **Lighting:** Heavy use of darkness, fog, and shadow. Torches and light
  sources are critical. The player should often be unable to see what lies
  ahead. Light is safety; darkness is threat.
- **Detail:** The world is full of environmental storytelling. Collapsed
  buildings, scattered bones, abandoned camps, claw marks on walls, bloodstains
  on floors. The world communicates its history through what you see, not what
  you're told.
- **Gore & violence:** Present but not gratuitous. Combat feels weighty and
  brutal. Enemies don't evaporate — they crumple, shatter, or dissolve depending
  on what they are.

### Reference Touchstones

- **Diablo 2** — The gold standard for isometric dark atmosphere. Marrowfall
  aims for the feeling of entering the Arcane Sanctuary or the Durance of Hate
  for the first time.
- **Dark Souls** — Environmental storytelling, loneliness, hostile world that
  doesn't explain itself.
- **Darkest Dungeon** — Oppressive stress and dread. The feeling that the world
  is actively wearing you down.
- **Bloodborne** — Gothic horror aesthetic, visceral combat, a world gone deeply
  wrong.

### Audio Direction

- **Minimal music.** Long stretches of silence or ambient sound. Music is used
  sparingly and intentionally — a faint melody when approaching a camp, a
  low drone in deep dungeons, a surge during a boss encounter. The absence of
  music makes its presence powerful.
- **Environmental sound design is paramount.** Dripping water, distant groans,
  wind through ruins, the crunch of bone underfoot, the scrape of metal on
  stone. The player should feel the world through their ears.
- **Silence as a tool.** Some of the scariest moments should be completely
  silent. No music, no ambient noise. Just the player's footsteps — and then
  something else's.

### UI Philosophy

- **Minimal HUD.** Health, stamina, and active effects. No minimap cluttered
  with icons. No quest tracker. No damage numbers floating off enemies.
- **Immersion-first.** Information the player needs is communicated through the
  game world where possible (a cracked weapon model signals low durability,
  labored breathing signals low stamina) rather than through UI elements.
- **Inventory management matters.** The player has limited carry capacity.
  Choosing what to bring and what to leave creates meaningful decisions,
  especially on long dungeon runs.

---

## Target Audience & Comparable Titles

### Who Is This For?

Players who:
- Loved Diablo 2's atmosphere but wished it went darker
- Miss Ultima Online's classless skill-by-use freedom
- Enjoy the punishing combat of Dark Souls / early PoE2 beta
- Want crafting to be *the* core system, not an afterthought
- Prefer sandbox games where they set their own goals
- Value earned progression over random loot
- Aren't afraid of permadeath

### Comparison Matrix

| Feature | Marrowfall | Diablo 2 | Ultima Online | Path of Exile 2 | Valheim |
|---------|-----------|----------|---------------|-----------------|---------|
| **Perspective** | Isometric | Isometric | Isometric | Isometric | Third-person |
| **Classes** | None | 7 fixed | None | Multiple | None |
| **Progression** | Skill-by-use | Level + skill tree | Skill-by-use | Level + passive tree | Skill-by-use |
| **Loot system** | Craft everything | Enemy drops | Mixed | Enemy drops | Craft everything |
| **Combat feel** | Punishing, deliberate | Fast, power-fantasy | Varied | Punishing (early) → fast | Stamina-based |
| **World gen** | Seed-defined, endless | Mostly procedural | Fixed | Fixed | Procedural |
| **Tone** | Bleak, hopeless | Dark but heroic | Varied | Dark | Cozy survival |
| **Story** | None (sandbox) | Linear campaign | Sandbox | Campaign | Sandbox |
| **Permadeath** | Optional hardcore | Optional hardcore | Optional | Softcore/Hardcore | No |
| **Multiplayer** | No | Yes | Yes (MMO) | Yes | Yes (co-op) |

**Marrowfall fills a gap:** no existing game combines Diablo 2's atmosphere,
Ultima Online's classless progression, PoE2's punishing combat, and a
crafting-first zero-loot-drop economy in a single-player sandbox.

---

## Sample Play Session

*The following describes approximately 30 minutes of freeform gameplay to
illustrate how the systems work together.*

---

You load into your character, a survivor 12 hours into a playthrough on Hardcore
mode. You are at the camp you cleared and claimed two sessions ago, an hour's
walk out from where you woke up. You have a Fine Iron Sword (Blacksmithing 45
craft), a Bone Buckler (your first real shield, made from skeleton remains),
leather armor you stitched together from wolf hides, and a small stack of
healing salves (Alchemy 22).

A caravan is parked nearby, the second one you have found. The trader has a
recipe scroll: **Reinforced Leather Armor**. It costs 80 gold. You have 53. You
make a note of where the caravan is and head out.

Your goal today: the cave mouth you spotted north-east last session, further out
than you have been. You need silver to upgrade your sword, and the bare rock out
that way is where silver lives.

You leave the camp. Fog rolls in. The ambient sound shifts, the murmur of the
fire fading behind you, replaced by wind and silence.

Ten minutes into the lowlands, you spot a cluster of skeletons near a collapsed
farmhouse. Three of them, armed with rusted weapons. In normal mode, you might
charge in. In Hardcore, you circle wide, studying their patrol pattern. You
approach from the side, engage the isolated one first.

Combat is tense. Your sword connects — the skeleton staggers. You dodge-roll
away from its swing, spending stamina. The other two notice and approach. You
back up, manage your stamina, pick your swings carefully. Two minutes later,
three skeletons lie shattered on the ground.

You harvest: **12 bone fragments, 4 bone dust, 2 rusted metal scraps**. You
pocket everything. Your Swordsmanship ticked up slightly from the fight. Your
Skinning skill doesn't apply here — these are skeletons, not beasts.

You press on. Near the mine entrance, you find a **wild herb patch** —
nightshade, useful for potions. You gather it. Herbalism +0.2.

The cave is dark. You light a torch (crafted from wood + cloth + tallow). The
torch doesn't last forever, and you have three. Every cave has its own generated
interior, so nobody has ever walked this layout, on this character or any other.

First chamber: empty. Collapsed supports, old mining carts. You spot an **iron
ore vein** in the wall. You pull out your pickaxe and mine. Mining +0.3. You get
6 iron ore.

Second chamber: you hear movement. A **cave troll** emerges from the darkness.
This is a serious fight. Trolls hit hard, have high health, and regenerate
slowly. On Hardcore, this could end your run.

You have a choice: fight or retreat. You've been saving your two **venom-tipped
arrows** (crafted last session from spider venom + iron arrowheads). You switch
to your bow. The first arrow hits — the troll staggers, poison ticking. You
dodge its charge. The second arrow lands. You switch to sword and close in while
it's slowed by venom, managing your stamina carefully between strikes and
dodges.

The troll falls. You harvest: **troll hide (3), troll fat (2), massive bone
(1)**. Troll fat is an alchemy ingredient you've been wanting. The massive bone
might be useful for a new weapon, and you'll check recipes back at camp.

Deeper in, you find what you came for: a **silver ore vein**. Mining +0.5 (new
material type bonus). You get 4 silver ore.

In the same chamber, half-hidden behind rubble: a **locked chest**. Your
Lockpicking is only 15. You try it. Fail. Try again. Fail. The lock is beyond
your current skill. You mark the location mentally and move on. Something to
come back for when your Lockpicking improves.  Torch is getting low. You light
your second one and decide to head back. On the way out, you take a different
tunnel and discover a **small hidden room** with a bookshelf. On it: a torn page
from an alchemical text. It's a recipe: **Troll Fat Salve**, a powerful healing
item that uses troll fat. Your Alchemy is just barely high enough to attempt it.

You walk back to your camp. At the forge you built there, you smelt your iron
ore into ingots. At the alchemy table, you craft two Troll Fat Salves using the
new recipe. At the tanning rack, you process the troll hide.

You're 27 gold short of that Reinforced Leather Armor recipe. You sell some
excess bone fragments to the trader, if the caravan is still there. Still 14
short. You decide to save it for next time.

You log off at your fire, the only light for a kilometre. Tomorrow: back into
that cave, the locked chest, and maybe enough silver to forge something new.

No quest told you to do any of this. You decided.

---

*This is Marrowfall.*
