#!/usr/bin/env python3
"""Generate bornpets sprite-sheet frames with the OpenAI image API.

Pipeline (one frame at a time, so a bad frame can be regenerated alone):

    gen       panda frame -> upscaled reference PNG -> /v1/images/edits
              -> raw 1024x1024 RGBA PNG in <work>/raw/
    quantize  raw PNG -> alpha-trim -> rescale to the panda frame's bbox
              -> 3-colour palette (black/white/red) + transparency
              -> <work>/frames/
    assemble  <work>/frames/ -> paste into the pet's sheet PNG
    state     the pet's OWN shipped frames (idle + a few more) as references
              -> one new state frame -> cut back to the exact 152x75 frame
    simplify  merge enclosed specks of one colour in a frames/ PNG

BORNPETS_PET picks the pet (default: badger); its prompts live in PROFILES.
The API key comes from --key-file, OPENAI_API_KEY_FILE, OPENAI_API_KEY or
~/.config/openai/api-key (see api_key). Network calls go through `curl`
(multipart upload); no third-party Python packages beyond Pillow.

    python3 scripts/bornpets_ai_frames.py gen --only badger_idle_0
    python3 scripts/bornpets_ai_frames.py quantize --only badger_idle_0
    python3 scripts/bornpets_ai_frames.py assemble
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import subprocess
import sys
from pathlib import Path

from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bornpets_art_check import ART_PETS, FRAME, STATES, TO_BADGE, decode_pcx  # noqa: E402
from bornpets_sheet import BLACK, RED, TRANSPARENT, WHITE, load_sequences  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
PANDA_SHEET = REPO / "assets/bornpets-sprites-panda.png"
PANDA_CFG = REPO / "assets/bornpets-sponsors-panda.json5"
# Which pet to work on. Its prompts live in PROFILES below; its sheet and config
# follow the asset naming convention.
PET = os.environ.get("BORNPETS_PET", "badger")
# PET_SHEET / PET_CFG are defined after PROFILES (a profile may name its own).

API_URL = "https://api.openai.com/v1/images/edits"
# gpt-image-1 (which the badger was drawn with) has been retired, and gpt-image-2
# is deprecated in favour of the 2.6 models.
MODEL = os.environ.get("BORNPETS_MODEL", "gpt-image-2.6-sunburst")
# Set by the global --key-file (or by another script); api_key() falls back to it.
KEY_FILE: str | None = None

# gpt-image-1 only renders these canvases. Pick the one closest to the frame's
# aspect so the model doesn't compose for a square and leave the wide 152x75
# frame half empty.
GEN_SIZES = ((1024, 1024), (1536, 1024), (1024, 1536))


def gen_size(frame_w: int, frame_h: int) -> tuple[int, int]:
    target = frame_w / frame_h
    return min(GEN_SIZES, key=lambda s: abs(s[0] / s[1] - target))

# The one frame that is generated standalone, hand-checked, and then passed as a
# character sheet into every other frame's request.
ANCHOR_ID = f"{PET}_idle_0"

# Escape hatch for a frame that needs a different camera distance from the
# anchor's. Empty in practice: dropping to low fidelity was tried on the battle
# stand and did not change the framing, it only cost character consistency.
FIDELITY_OVERRIDES: dict[str, str] = {}

# Chroma key for the generated background. Magenta is outside the badge palette,
# so keying it out can never eat real artwork.
KEY = (255, 0, 255, 255)


def anchor_path(work: Path) -> Path:
    return work / "anchor.png"

# ---------------------------------------------------------------------------
# Prompt construction
# ---------------------------------------------------------------------------

# The anchor image carries the character; the prompt carries the scene.
#
# Earlier revisions also fed the matching panda frame in as a pose reference.
# That produced panda clones. The other pets in this game do not work that way
# — the slug sleeps in a bed under a moon and a 04:00 clock, and lounges in a
# deck chair between two speakers, while the panda just lies down. Each pet gets
# its own read on the same action, so the badger does too.
BADGER_IDENTITY = (
    "IMAGE 1 is the CHARACTER SHEET for this game's badger pet. It defines who "
    "the character is and must be reproduced exactly: a kawaii chibi European "
    "badger with an oversized white head that tapers to a pointed muzzle with a "
    "small black nose; two broad BLACK STRIPES that start beside the nose, run "
    "up over each eye and continue back over the crown to the ears, with a "
    "white blaze between them and white cheeks outside them; big round shiny "
    "eyes inside the stripes, each ringed in white with a white highlight dot; "
    "plain white cheeks with NO red blush - nothing red on the face; small "
    "rounded black ears with white "
    "rims; a stocky SOLID BLACK body and short legs with a WHITE belly patch; "
    "a short bushy tail; and the line weight. The EYES always sit INSIDE the "
    "black stripes and must always be visible there: an open eye has a white "
    "ring around its black pupil, and a closed, squinting or sleeping eye is "
    "drawn as a bold WHITE curved line or white X on the black stripe - never "
    "black on black, and never moved out onto the white cheeks. It must never look like a panda: "
    "no round black eye patches, no white body. Anyone must recognise it as "
    "the same badger in every frame."
)

BADGER_FREEDOM = (
    "You are NOT copying image 1's pose. Compose this frame yourself: choose the "
    "badger's posture, expression, camera framing and any props or background "
    "objects that best tell the viewer what is happening. Be playful and "
    "characterful - this is a virtual-pet game, and the scene should read "
    "instantly at a glance. Badger-native props are encouraged where they suit "
    "the scene: earthworms and grubs for food, its underground sett, loose soil "
    "and digging. Only add night-time or moon imagery when the scene brief "
    "actually calls for it - do not put a moon in every frame."
)

# The badge renders transparency as unlit background, so a sprite only reads as
# a silhouette if it is white-dominant. A naturalistic dark-bodied badger
# disappears once it is scaled down to 152x75.
STYLE_TEMPLATE = (
    "Style: chunky hard-edged 1-bit retro pixel art for a monochrome e-ink "
    "virtual-pet game, matching image 1's line weight exactly. Use ONLY pure "
    "black (#000000), pure white (#FFFFFF) and pure red (#FF0000) for the "
    "artwork - no grey, no other colours, no anti-aliasing, no shading, no "
    "gradients. The art is WHITE-DOMINANT: shapes are filled solid white and "
    "black is used only for outlines, {markings} and small "
    "details. Red is a bold accent used sparingly, in chunky solid shapes "
    "at least 3 pixels thick - never thin red hairlines, they disappear when the "
    "sprite is scaled down.\n\n"
    "THERE IS NO BACKGROUND AND NO SCENERY. Do not draw a sky, ground, floor, "
    "horizon, wall, room, landscape, backdrop or panel border. The character and "
    "its props float on the empty magenta chroma key, which fills every pixel "
    "that is not the character or a prop and is used nowhere else. Props are "
    "small discrete objects placed next to or touching the character, drawn as "
    "white shapes with thin black outlines.\n\n"
    "NEVER fill a large area with solid black. Black is only for thin outlines, "
    "{markings}, and small details. Any "
    "black shape bigger than a few pixels across is wrong - a black night sky, a "
    "black patch of ground or a black floor mat ruins the sprite. Do not add "
    "dithering, stippling, speckles or scattered dots anywhere. This rule is "
    "about scenery and props; a black area the character sheet itself has is "
    "part of the character and stays black.\n\n"
    "Compose for a WIDE LANDSCAPE frame and use the full width of the canvas - "
    "the {pet} does not have to be centred, and props may sit to its left or "
    "right, but leave no large empty margins."
)


def style() -> str:
    return STYLE_TEMPLATE.format(pet=PROFILE["noun"], markings=PROFILE["markings"])


# Per-frame scene briefs, keyed by "<sequence>_<frame index>". These describe the
# ACTION, not a pose to copy; the model stages each one itself.
BADGER_POSES: dict[str, str] = {
    "badger_idle_0": "Idle. The badger sits upright facing the viewer, calm and content, front paws resting in front of it.",
    "badger_happy_0": "Delighted, at its own little party. The badger beams, paws up in celebration. Above it hangs a string of bunting - little triangle flags alternating SOLID RED and white - and a few bold red hearts and chunky confetti squares pop around it.",
    "badger_happy_1": "Delighted, second animation frame: the badger bounces higher with an even bigger grin under the SAME red-and-white bunting from the previous frame, the hearts and confetti scattered to the other side.",
    "badger_critical_sick_0": "Gravely ill. The badger sits facing the viewer with its eyes screwed shut - each shut eye a bold WHITE squiggle on its black stripe so it shows clearly - a red ice pack balanced on its head and a thermometer sticking out of its mouth. Nothing else in the frame.",
    "badger_critical_sick_1": "Gravely ill, second animation frame: the same sitting badger shivers, eyes screwed shut as white squiggles on its black stripes. The SAME SOLID RED ice pack from the previous frame is sliding off its head. A thermometer sticks out of the corner of its MOUTH, with a bold red bulb at the end. A few small shiver lines beside it. Nothing else in the frame.",
    "badger_critical_tired_0": "Utterly exhausted and SLEEPY - not sad, not crying, not depressed. The badger sits slumped, swaying on the spot, mouth wide open in a huge yawn, heavy drooping eyelids drawn as white lines on its black stripes, one paw propping up its heavy head. A single Z floats beside it. Nothing else in the frame.",
    "badger_critical_hungry_0": "Starving. The badger stares mournfully into a completely empty food bowl, ribs showing, a thought bubble full of fat red earthworms above it.",
    "badger_critical_hungry_1": "Starving, second animation frame: the badger paws desperately at the empty bowl with a huge bold red exclamation mark beside its head. The bowl is the SAME as in the previous frame: a plain round WHITE bowl with a thick black outline and one black paw print on its front, completely empty.",
    "badger_critical_drained_0": "Completely out of energy. The badger droops like a deflated balloon, eyes half shut, with a big red battery icon showing empty next to it.",
    "badger_critical_drained_1": "Completely out of energy, second animation frame: the badger has flopped down onto its tummy, chin resting on the ground, eyes drowsily half open as white lines, a small weary sigh puff by its nose - exhausted but clearly ALIVE, not dead: no X eyes, no tongue hanging out, not on its back. The SAME empty red battery icon from the previous frame flashes beside it.",
    "badger_warning_sick_0": "Coming down with something. The badger sits wrapped in a small blanket, red-nosed, sneezing into a tissue.",
    # These three are normally produced together by `strip badger_tired`, which
    # draws the whole cycle in one image so the frames match each other. The
    # briefs below exist so a single frame can still be regenerated on its own.
    "badger_warning_tired_0": "Sleepy. The badger yawns hugely, one paw rubbing an eye, a nightcap slipping down over its ear.",
    "badger_warning_tired_1": "Sleepy, second animation frame: the badger's head has nodded forward, eyes half shut, the nightcap slid further down over one ear.",
    "badger_warning_tired_2": "Sleepy, third animation frame: the badger is STILL SITTING UPRIGHT and still awake, just barely - fighting to stay awake. Its head nods heavily forward, eyelids drooping almost shut as white lines on its black stripes, the SAME solid red nightcap from the previous frame slipping down over one eye, one small Z beside it. It is tired, NOT asleep: not lying down, no pillow.",
    "badger_warning_hungry_0": "Peckish. The badger looks hopefully at the viewer, one paw on a rumbling belly, a thought bubble with a single red earthworm.",
    "badger_warning_hungry_1": "Peckish, second animation frame: the badger licks its lips and points at its empty bowl - a plain round WHITE bowl with a thick black outline and one black paw print on its front - while its thought bubble now holds a whole bowl of red worms.",
    "badger_warning_drained_0": "Running low. The badger slumps forward with heavy eyelids, a half-empty red battery icon beside it.",
    "badger_warning_drained_1": "Running low, second animation frame: the badger sags further, propping itself up on one wobbling paw, eyelids heavy. Beside it is ONLY the SAME half-empty red battery icon from the previous frame - no worms, no speech bubbles, no other creatures or objects.",
    "badger_warning_miserable_0": "Thoroughly miserable. The badger sits hunched and scowling under its own small rain cloud, ears flat.",
    "badger_feeding_0": "Feeding. The badger tucks happily into a big bowl heaped with fat red earthworms, cheeks bulging, worms dangling from its mouth. The bowl is a plain round WHITE bowl with a thick black outline and one black paw print on its front.",
    "badger_feeding_1": "Feeding, second animation frame: the meal is finished. The badger sits back with a full round belly and a satisfied grin, eyes closed happily. Beside it stands the SAME bowl as in the previous frame - the plain round white bowl with the black paw print on its front - now licked completely clean and EMPTY. Same bowl shape and decoration, no hearts on it.",
    "badger_healing_0": "Being healed. The badger perks up as a bold red medical cross glows above it, a bandage wrapped round one paw.",
    "badger_healing_1": "Being healed, second animation frame: the badger flexes the newly bandaged paw proudly, the red cross drifting off to one side.",
    "badger_relaxing_0": "Relaxing on holiday. The badger lounges back in a deck chair whose fabric has bold SOLID RED and white stripes, paws behind its head, thoroughly at ease. Beside the chair stands a small palm tree with white leaves outlined in black. Nothing else in the frame.",
    "badger_relaxing_1": "Relaxing on holiday, second animation frame: still in the SAME deck chair - its fabric keeps the bold SOLID RED and white stripes - now sipping a drink through a straw, a couple of bold red hearts floating up. The SAME small palm tree stands beside the chair.",
    "badger_relaxing_2": "Relaxing on holiday, third animation frame: the drink is finished and the badger has slid far down the SAME deck chair, dozing with eyes closed, one paw trailing on the ground. The chair fabric keeps its bold SOLID RED and white stripes, and the SAME small palm tree stands beside it.",
    "badger_playing_0": "Playing tennis. The badger swings a tennis racket with a bold red frame, mid-stroke, grinning with delight, its face fully visible. A small white tennis ball with a black outline flies off to one side, clear of the face, with motion ticks behind it. No net, no court, no ground.",
    "badger_playing_1": "Skateboarding. The badger rides a skateboard with a bold solid red deck and white wheels, arms out for balance, mid-ollie with the board just off the ground, grinning with delight. A few motion ticks behind it. No ramp, no ground.",
    "badger_sleeping_0": "Fast asleep at last, continuing the tired animation: the badger lies curled up with its head resting on a small white pillow, eyes closed as white curved lines, the SAME solid red nightcap from the tired frames flopped over one ear, two Z's rising - white letters with a thick black outline. Nothing else in the frame.",
    "badger_sleeping_1": "Fast asleep, second animation frame: the SAME badger on the SAME small white pillow, in the SAME solid red nightcap, has snuggled deeper into the pillow with a content little smile, eyes closed as white curved lines, the Z's drifting higher. NO bubble at its mouth or nose. Nothing else in the frame.",
    "badger_sleeping_2": "Fast asleep, third animation frame: the SAME badger, pillow and solid red nightcap, now hugging the pillow with both front paws, eyes closed as white curved lines, one Z floating away. NO bubble at its mouth or nose. Nothing else in the frame.",
    "badger_leaving_0": "Leaving. The badger stands facing the viewer waving goodbye with one paw, a small bindle on a stick over its shoulder. Nothing else in the frame.",
    "badger_leaving_1": "Leaving, second animation frame: the badger has turned around and is walking away from the viewer, seen from BEHIND, still carrying the SAME bindle on a stick from the previous frame over its shoulder - the knapsack bundle hangs clearly visible behind its head. Its back is towards us, so draw the back of its head with the two black stripes running up over the top of the skull between the ears and the white blaze between them, its solid black back, a short bushy tail, and its rear paws mid-stride. It must read clearly as a badger walking off. Nothing else in the frame.",
    # "almost entirely empty" was read as licence to draw the badger anyway, so the absence is now the subject: a burrow up for rent, tenant long gone.
    # The badger kept turning up in "an almost empty frame", and a "dark burrow hole" is invisible on a transparent background, so the mound has to be the white
    # shape and the tunnel the black one.
    "badger_gone_0": "THE BADGER IS NOT IN THIS FRAME AT ALL - no animal, no head, no paws, no tail, no eyes peeking out of the hole. The pet has moved out and its home is up for rent. Composition, like a signposted but deserted site:\n(1) LEFT AND CENTRE: a solid WHITE bank of earth with a lumpy uneven top edge, sitting along the bottom of the frame. Cut into the face of that bank, low down, is the badger sett entrance: a BLACK hole that is clearly WIDER THAN IT IS TALL - a low, squat, ragged opening, never a tall dome or archway. The bank is white so it stays visible; the hole is the only black shape in it. A few white lumps of dug-out soil are scattered in front of the hole.\n(2) RIGHT FOREGROUND: a wooden signboard on a post leaning at a slight angle, stuck into the ground and standing taller than the bank - a white rectangular plank with the words FOR RENT in black pixel capitals.\n(3) A couple of grass tufts on top of the bank.\nNothing else at all. The sett is empty.",
    "badger_hibernating_0": "Hibernating. The badger sits tucked up fast asleep with a small blanket round its shoulders, head drooping and eyes closed, a few small white snowflakes drifting around it. Nothing else in the frame.",
    "badger_hatching_0": "Hatching, first frame: a cracked white egg with the cub's striped snout just breaking through the shell. Nothing else in the frame.",
    "badger_hatching_1": "Hatching, second frame: the cub's head is out of the broken egg, the top of the shell perched on its head like a cap. Nothing else in the frame.",
    "badger_hatching_2": "Hatching, third frame: the cub bursts up out of the lower half of the shell, paws in the air, delighted. Nothing else in the frame.",
    "badger_hatching_3": "Hatching, final frame: the cub is fully hatched and sits upright and pleased with itself, the empty broken eggshell lying beside it. Nothing else in the frame.",
    # "full body" alone kept returning a torso-up crop; the zoom-out has to be stated as framing (small figure, gap above and below) to actually take.
    # Four attempts to get a full-length standing pose (explicit "head to toe", zoom-out framing, dropping the word "portrait", low input_fidelity) all came back torso-up and progressively messier. gpt-image-1 renders a chibi animal head-and-torso in a square frame regardless. This is the clean version; the panda's own battle frame is framed much the same way.
    "battle_badger_stand_0": "Square battle sprite. The badger stands facing the viewer in a bold red headband, front paws raised in big red boxing gloves, face clearly showing its two black head stripes and a determined glare. Nothing else in the frame - no floor, no mat, no arena.",
    "battle_badger_won_0": "Square battle portrait. The badger celebrates victory facing the viewer, one paw thrust up, wearing its red headband, a bold red medal on its chest. Nothing else in the frame.",
    # The dizzy-spiral eyes kept overriding the head markings and the face came back blank white, so the stripes are stated first, before the defeat.
    # Spiral "dizzy" eyes reliably displaced the head stripes - the model drew one or the other, never both - so this frame uses X-shaped KO eyes instead.
    "battle_badger_lost_0": "Square battle portrait showing the badger's whole body, not a close-up. Head markings exactly as on the character sheet: two thick BLACK STRIPES running from beside the nose up through each eye to the top of the head, a white blaze between them, white cheeks outside them. Each eye is a small white X knocked out of the black stripe. This badger has just lost a fight: it sits slumped, tongue out, red headband askew, three bold red stars circling above. Nothing else in the frame.",
    "badger_exercising_0": "Working out. In a bold red headband, the badger does a press-up, body angled and tongue out with effort. No mat, no floor.",
    "badger_exercising_1": "Working out, second animation frame: in the SAME bold red headband, the badger does bicep curls, a SMALL chunky dumbbell gripped firmly in EACH front paw, elbows bent so the dumbbells sit at shoulder height right in its paws - never floating above it. A few sweat drops fly. No worms, no mat, no floor, nothing else.",
    "badger_medicating_0": "Taking its medicine. The badger eyes a big red pill warily, holding it at arm's length.",
    "badger_medicating_1": "Taking its medicine, second animation frame: the badger swallows the pill with a grimace and a red cross puffs above its head.",
    "badger_drinking_0": "Tipsy. The badger has had one too many: it sways on wobbly legs, leaning at an angle, clutching a big bottle in one paw. Wonky half-lidded eyes drawn as white lines on its black stripes, a goofy lopsided grin, a 'HIC!' in chunky black pixel letters beside its head and a few small bubbles floating up. Nothing else in the frame.",
    "badger_ozempic_0": "Comically shrunken. The badger has become absurdly skinny and gaunt, holding an oversized red syringe, its skin hanging loose.",
    "badger_rehab_0": "In rehab. The badger sits sheepishly on a small stool clutching a red pill bottle, looking guilty.",
    "badger_rehab_1": "In rehab, second animation frame: the badger throws the red pill bottle away with a determined look, sparkles around it.",
    # Keep this one plain: the earlier life-drawing gag tripped the image API's
    # output moderation as sexual content. The joke lives in the sign text.
    "badger_onlypets_0": "A cheesy OnlyFans-style parody called ONLYPETS, kept cute and family-friendly. The badger strikes an over-the-top glamour pose, lounging on its side on a small heart-shaped cushion, chin on one paw, a red rose clamped in its mouth, heart-shaped sunglasses pushed up on its head, batting its eyes at a phone on a tripod. A round ring light stands beside it. A small sign reads ONLYPETS in black pixel capitals with a bold red heart. Nothing else in the frame.",
}


# Frames that must reuse a prop from an earlier frame get it as a second
# reference (see CONTINUITY).
BADGER_CONTINUES = {
    "badger_critical_drained_1": "badger_critical_drained_0",
    "badger_critical_hungry_1": "badger_critical_hungry_0",
    "badger_critical_sick_1": "badger_critical_sick_0",
    "badger_happy_1": "badger_happy_0",
    "badger_sleeping_0": "badger_warning_tired_1",
    "badger_sleeping_1": "badger_sleeping_0",
    "badger_sleeping_2": "badger_sleeping_1",
    "badger_exercising_1": "badger_exercising_0",
    "badger_warning_drained_1": "badger_warning_drained_0",
    "badger_warning_tired_2": "badger_warning_tired_1",
    "badger_feeding_1": "badger_feeding_0",
    "badger_leaving_1": "badger_leaving_0",
    "badger_relaxing_1": "badger_relaxing_0",
    "badger_relaxing_2": "badger_relaxing_1",
}


# ---------------------------------------------------------------------------
# Animation strips
# ---------------------------------------------------------------------------
#
# One request, one image, N panels. Generating each frame of an animation in its
# own request gives the model no way to see the previous frame, so consecutive
# frames drift: the props move, the line weight shifts, the pose resets. Asking
# for the whole cycle in a single image makes the model draw them as a set, and
# consistency between them comes for free.
#
# Panels are stacked vertically because the badge frame is wide (152x75). A
# 1024x1536 canvas splits into three 1024x512 cells, aspect 2.0 against the
# frame's 2.03 - so each panel crops to a frame with almost nothing to trim.
STRIP_SIZE = (1024, 1536)

BADGER_STRIPS: dict[str, dict] = {
    "badger_tired": {
        "size": STRIP_SIZE,
        "targets": [
            "badger_warning_tired_0",
            "badger_warning_tired_1",
            "badger_warning_tired_2",
        ],
        "subject": (
            "the badger getting steadily sleepier - a three-step tired animation "
            "that loops. In ALL THREE frames the badger stays SITTING UPRIGHT "
            "facing the viewer with its whole body and both front paws visible, "
            "its two BLACK HEAD STRIPES clearly drawn exactly as on the character "
            "sheet, and its nightcap FILLED SOLID RED. It never lies down, never "
            "collapses into a heap, and never loses its face markings"
        ),
        "panels": [
            "TOP PANEL - frame 1: the badger sits upright and yawns hugely, mouth wide open, one paw rubbing an eye, the solid red nightcap starting to slip down over one ear.",
            "MIDDLE PANEL - frame 2: the same badger, still sitting upright, same size and same place in its panel, has let its head nod forward, eyes now half shut, the solid red nightcap slid further down over the ear.",
            # Solid white Z's are invisible on the badge: transparent pixels are
            # skipped, so the sprite sits on white paper and an unoutlined white
            # shape has nothing to read against. The panda's Z's are white with a
            # black outline; these have to be too.
            "BOTTOM PANEL - frame 3: the same badger, STILL SITTING UPRIGHT, same size and same place in its panel, head drooped forward with eyes fully closed, the solid red nightcap flopped over one ear, and two Z's rising to one side. Draw the Z's as WHITE letters with a THICK BLACK OUTLINE around each one, like the rest of the artwork - never plain white letters with no outline.",
        ],
    },
}

# The cut is a blind slice at exact fractions of the canvas height, so the model
# has to keep every panel's art inside its own band. Stated as a hard rule and
# repeated as a margin instruction, because "three panels" alone gets read as
# licence to overlap them.
STRIP_STYLE = (
    "LAYOUT: this single image contains {n} separate animation frames stacked "
    "vertically, in equal horizontal bands, each exactly {frac} of the image "
    "height. Band 1 is the top {frac}, band {n} is the bottom {frac}.\n\n"
    "Each band is one complete WIDE LANDSCAPE frame of the animation. The art in "
    "a band must stay entirely inside that band: leave a clear magenta margin "
    "along the top and bottom edge of every band so nothing touches or crosses "
    "the boundary between two bands. This applies to the LAST band too - the "
    "bottom of the final frame's artwork must stop well short of the bottom edge "
    "of the image, never run off it. Every frame must show the whole {pet}, "
    "uncropped. Do NOT draw panel borders, frames, gutters, "
    "dividing lines, numbers or labels - the bands are separated by empty "
    "magenta only.\n\n"
    "The bands are frames of ONE animation of the SAME {pet}. Keep the {pet} "
    "the same size, drawn at the same scale, in the same position within its "
    "band, and with the same line weight in every band. Only what the brief says "
    "changes between frames may change - everything else must be pixel-identical "
    "from band to band, so the frames play as a smooth loop."
)


def strip_prompt(key: str) -> str:
    spec = STRIPS[key]
    n = len(spec["panels"])
    panels = "\n".join(spec["panels"])
    return (
        f"Draw a {n}-frame animation strip for a pixel-art virtual-pet game.\n\n"
        f"{IDENTITY}\n\n"
        f"{STRIP_STYLE.format(n=n, frac=f'1/{n}', pet=PROFILE['noun'])}\n\n"
        f"The animation shows {spec['subject']}.\n\n"
        f"{panels}\n\n"
        f"{FREEDOM}\n\n"
        f"{style()}"
    )


def badger_anchor_prompt() -> str:
    """Prompt for the one hand-approved frame every other frame references.

    The first badger was the panda frame re-marked, and it kept the panda's
    round head and white body, so it read as a panda. This one is drawn fresh:
    the panda frame (image 1) is only a style reference, and `anchor --ref`
    adds badger reference art (images 2+) for the anatomy.
    """
    return (
        "IMAGE 1 is a sprite from a pixel-art virtual-pet game, showing a "
        "panda. Use it ONLY as a style reference - its line weight, pixel "
        "scale, chunky outlines and small red accents. Do NOT draw a panda or "
        "anything panda-like. Any further images are reference drawings of a "
        "cartoon badger: follow them for the animal's anatomy and markings, "
        "not for their grey shading.\n\n"
        "Draw this game's badger pet as a cute KAWAII chibi European badger "
        "(Meles meles), standing upright on its hind legs facing the viewer:\n"
        "- An oversized head, wide at the top and tapering to a POINTED muzzle "
        "with a small black nose at the tip - a wedge, not a round panda face.\n"
        "- Two broad BLACK STRIPES, one each side: each starts beside the nose, "
        "runs up over the eye and continues back over the top of the head to "
        "the ear. A white blaze runs between them from the nose to the crown; "
        "the cheeks outside them are white.\n"
        "- Big round shiny eyes sitting inside the black stripes, each ringed "
        "in white so it shows, with a white highlight dot.\n"
        "- A tiny happy smile. Plain white cheeks: NO red blush, nothing red "
        "on the face - real badgers have none.\n"
        "- Small rounded BLACK ears with a white rim, set on the sides of the "
        "head where the stripes end.\n"
        "- A stocky body and short legs filled SOLID BLACK, with a WHITE belly "
        "patch down the front.\n"
        "- A short bushy tail poking out to one side.\n"
        "- Idle and content, front paws relaxed.\n"
        "Nothing else in the frame.\n\n"
        f"{style()}"
    )


# ---------------------------------------------------------------------------
# Sea star
# ---------------------------------------------------------------------------
#
# Unlike the badger, the sea star is not a re-marked panda: the body plan is
# different, so the panda frame is passed only as a style reference (line
# weight, palette) at low fidelity and the character comes from the prompt.
# The look follows a kawaii colouring-page starfish: big glossy eyes, blush
# cheeks, open smile.

SEASTAR_IDENTITY = (
    "IMAGE 1 is the CHARACTER SHEET for this game's sea star pet. It defines who "
    "the character is and must be reproduced exactly: a plump five-armed sea "
    "star (starfish) with soft rounded arm tips, a solid white body with a thick "
    "black outline, a short curved black crease line along each arm, two BIG "
    "round black eyes each with a white highlight dot, a bold red blush oval on "
    "each cheek, a wide open happy smile with a red tongue, and the line weight. "
    "Its five arms are its limbs: the top arm is the point of its head, the two "
    "side arms work as hands and the two lower arms as legs. Anyone must "
    "recognise it as the same sea star in every frame."
)

SEASTAR_FREEDOM = (
    "You are NOT copying image 1's pose. Compose this frame yourself: choose the "
    "sea star's posture, expression, camera framing and any props that best tell "
    "the viewer what is happening. Be playful and characterful - this is a "
    "virtual-pet game, and the scene should read instantly at a glance. "
    "Seaside props are encouraged where they suit the scene: mussels and clams "
    "for food, seashells, a strand of seaweed, a rock, a few small round "
    "bubbles drawn as white circles with a black outline. Only add night-time "
    "or moon imagery when the scene brief actually calls for it - do not put a "
    "moon in every frame."
)


def seastar_anchor_prompt() -> str:
    return (
        "IMAGE 1 is a sprite from a pixel-art virtual-pet game, showing a white "
        "panda. Use it ONLY as a style reference - its line weight, pixel scale, "
        "white-filled shapes with black outlines and small red accents. Do NOT "
        "draw the panda or anything panda-like.\n\n"
        "Draw this game's NEW pet instead: a cute kawaii five-armed SEA STAR "
        "(starfish) standing upright and facing the viewer:\n"
        "- A plump star shape with soft, rounded arm tips, filled solid white "
        "with a thick black outline.\n"
        "- A short curved black crease line along each arm.\n"
        "- Two BIG round black eyes, each with one white highlight dot.\n"
        "- A bold red blush oval on each cheek, under the eyes.\n"
        "- A wide open happy smile showing a red tongue.\n"
        "- Idle and content: standing on its two lower arms, side arms relaxed.\n"
        "Nothing else in the frame.\n\n"
        f"{style()}"
    )


SEASTAR_POSES: dict[str, str] = {
    "seastar_idle_0": "Idle. The sea star stands upright on its two lower arms facing the viewer, calm and content, side arms relaxed, smiling.",
    "seastar_happy_0": "Delighted. The sea star beams with its eyes squeezed into happy arcs, side arms thrown up in celebration, two or three bold chunky red hearts popping around it.",
    "seastar_happy_1": "Delighted, second animation frame: the sea star spins in a happy cartwheel, tilted at a jaunty angle with an even bigger grin, hearts scattered on the other side.",
    "seastar_critical_sick_0": "Gravely ill. The sea star sits slumped facing the viewer with its eyes screwed shut and its arms hanging limp, a red ice pack balanced on its top arm and a thermometer sticking out of its mouth. Nothing else in the frame.",
    "seastar_critical_sick_1": "Gravely ill, second animation frame: the same slumped sea star shivers, the ice pack slides off its top arm, and a small bold red thermometer reading spikes beside it.",
    "seastar_critical_tired_0": "Utterly exhausted. The sea star has toppled over and lies flat on the ground, arms splayed out limp, tongue lolling out and eyes shut, too tired to move. Nothing else in the frame.",
    "seastar_critical_hungry_0": "Starving. The sea star stares mournfully into a completely empty food bowl, arms wilting, a thought bubble full of plump mussels above it.",
    "seastar_critical_hungry_1": "Starving, second animation frame: the sea star scrapes desperately at the empty bowl with one arm, a huge bold red exclamation mark beside its head.",
    "seastar_critical_drained_0": "Completely out of energy. The sea star droops, its arms sagging down like wilted petals, eyes half shut, with a big red battery icon showing empty next to it.",
    "seastar_critical_drained_1": "Completely out of energy, second animation frame: the sea star has flopped over onto its side, arms splayed, the empty red battery icon flashing.",
    "seastar_warning_sick_0": "Coming down with something. The sea star sits wrapped in a small blanket, sniffly, sneezing into a tissue held in one arm.",
    # Normally produced together by `strip seastar_tired`; the briefs let a
    # single frame be regenerated on its own.
    "seastar_warning_tired_0": "Sleepy. The sea star yawns hugely, one arm rubbing an eye, a red nightcap slipping down over its top arm.",
    "seastar_warning_tired_1": "Sleepy, second animation frame: the sea star's top arm has nodded forward, eyes half shut, the nightcap slid further down.",
    "seastar_warning_tired_2": "Sleepy, third animation frame: the sea star's top arm has drooped right down, eyes fully closed, nightcap flopped over, two Z's rising to one side.",
    "seastar_warning_hungry_0": "Peckish. The sea star looks hopefully at the viewer, one arm on its rumbling middle, a thought bubble with a single mussel.",
    "seastar_warning_hungry_1": "Peckish, second animation frame: the sea star licks its lips and points one arm at the empty bowl, the thought bubble now holding a whole pile of mussels.",
    "seastar_warning_drained_0": "Running low. The sea star slumps with heavy eyelids and drooping arms, a half-empty red battery icon beside it.",
    "seastar_warning_drained_1": "Running low, second animation frame: the sea star has sagged down sideways, leaning heavily on one bent, wobbling arm, eyes half shut, body tilted at an angle. The SAME red low-battery icon from the previous frame is still beside it.",
    "seastar_warning_miserable_0": "Thoroughly miserable. The sea star sits hunched and frowning under its own small rain cloud, side arms crossed.",
    "seastar_feeding_0": "Feeding. The sea star happily hugs a big open mussel shell with its arms and slurps up the red mussel inside, cheeks bulging.",
    "seastar_feeding_1": "Feeding, second animation frame: the meal is finished. The sea star leans back with a full round middle, eyes closed in a satisfied grin, patting its tummy with one arm. Beside it lies the SAME big mussel shell from the previous frame, now wide open and completely EMPTY - no red mussel left inside. No pile of extra shells.",
    "seastar_healing_0": "Being healed. The sea star perks up as a bold red medical cross glows above it, a bandage wrapped round the tip of one arm.",
    "seastar_healing_1": "Being healed, second animation frame: the sea star proudly shows off the bandaged arm, its tip regrown good as new, the red cross drifting off to one side.",
    "seastar_relaxing_0": "Relaxing. The sea star lounges back in a red-and-white striped deck chair, side arms behind its head, thoroughly at ease. Nothing else in the frame.",
    "seastar_relaxing_1": "Relaxing, second animation frame: still in the deck chair, sipping a drink through a straw, with a couple of bold red hearts floating up.",
    "seastar_relaxing_2": "Relaxing, third animation frame: the drink is gone and the sea star has dozed off, slumped far down and sideways in the SAME deck chair as in the previous frame, eyes closed, mouth open, one arm dangling over the side to the ground. The chair's fabric keeps its bold SOLID RED and white stripes.",
    "seastar_playing_0": "Playing. The sea star's face is fully visible and unobstructed, grinning with delight. It bats a bold red-and-white beach ball that flies off to ONE SIDE, clearly clear of the face - the ball must never cover or overlap the face. Motion ticks trail behind the ball. No ground, no sand.",
    # The sleeping row plays straight after the tired row, whose sea star wears a
    # red nightcap; without the hat stated here (and the previous frame passed in
    # via SEASTAR_CONTINUES) the hat vanished the moment it fell asleep.
    "seastar_sleeping_0": "Fast asleep at last, continuing the tired animation. The sea star sits upright facing the viewer, its top arm tipped forward and drooping, eyes closed as two curved lines, a small contented smile, red cheek blush. It is STILL WEARING the SAME nightcap from image 2 on its top arm: a floppy cone filled SOLID RED with a thick black outline, its tip flopped down over to one side ending in a round SOLID RED pom-pom. Three Z's rise to one side, each a white letter with a thick black outline. A seashell or two may sit beside it. Nothing else in the frame.",
    "seastar_sleeping_1": "Fast asleep, second animation frame: the SAME sitting sea star in the SAME solid red nightcap with its red pom-pom (exactly as in image 2) has slumped a little further forward, eyes closed as curved lines, a round white snoring bubble with a black outline growing at its mouth, the Z's drifting higher. The SAME seashells and bubbles from image 2 stay where they were, so the sea star keeps its size and place in the frame. Nothing else in the frame.",
    "seastar_sleeping_2": "Fast asleep, third animation frame: the SAME sitting sea star in the SAME solid red nightcap with its red pom-pom (exactly as in image 2) jolts slightly as the snoring bubble pops - where the bubble was, three or four SHORT CHUNKY black burst dashes, each at least 3 pixels thick (thin hairlines vanish when scaled down) - still drooping with eyes closed as curved lines. The SAME three Z's from image 2 still rise above it, white letters with a thick black outline. The nightcap stays on its top arm. The SAME seashells from image 2 stay where they were, so the sea star keeps its size and place in the frame. Nothing else in the frame.",
    "seastar_leaving_0": "Leaving. The sea star stands facing the viewer, sad and disappointed that things did not work out: eyes downcast, a single tear, mouth a small wobbly frown, one arm raised in a half-hearted goodbye wave, a small bindle on a stick over its shoulder. Not smiling. Nothing else in the frame.",
    "seastar_leaving_1": "Leaving, second animation frame: the sea star has turned away and is cartwheeling off to one side, seen from BEHIND: its back is plain white with a scattering of small round bumps and no face, the bindle swinging from one arm. It must read clearly as the sea star leaving. Nothing else in the frame.",
    "seastar_gone_0": "THE SEA STAR IS NOT IN THIS FRAME AT ALL - no star, no arms, no face, no eyes peeking out. The pet has moved out and its home is up for rent. Composition, like a signposted but deserted site:\n(1) LEFT AND CENTRE: a solid WHITE rock with a lumpy uneven top, sitting along the bottom of the frame. Low in its face is a BLACK crevice that is clearly WIDER THAN IT IS TALL - the sea star's old home. A couple of white seashells and a strand of seaweed lie beside the rock.\n(2) RIGHT FOREGROUND: a wooden signboard on a post leaning at a slight angle, standing taller than the rock - a white rectangular plank with the words FOR RENT in black pixel capitals.\n(3) Two or three small bubbles rising, white circles with a black outline.\nNothing else at all. The home is empty.",
    "seastar_hibernating_0": "Hibernating. The sea star sits tucked up fast asleep under a small blanket, eyes closed, a few small white snowflakes drifting around it. Nothing else in the frame.",
    "seastar_hatching_0": "Hatching, first frame: a cracked white egg with the rounded tip of one sea star arm just poking through the shell. Nothing else in the frame.",
    "seastar_hatching_1": "Hatching, second frame: the baby sea star's face is out of the broken egg, the top of the shell perched on its top arm like a cap. Nothing else in the frame.",
    # "the shell" was read as a seashell - this is a sea creature - so the egg has to be named outright.
    "seastar_hatching_2": "Hatching, third frame: the baby sea star bursts up out of the lower half of a cracked white EGGSHELL - a plain oval bird-style egg with a jagged zigzag broken rim, the same egg as in the earlier hatching frames. It is NOT a seashell, clam, scallop or oyster: no ridges, no fan shape. Arms in the air, delighted. Nothing else in the frame.",
    "seastar_hatching_3": "Hatching, final frame: the baby sea star is fully hatched and stands pleased with itself. Scattered on the ground around its lower arms lie four or five fragments of broken white eggshell with jagged zigzag edges - no whole egg, no egg cup, no seashells. Nothing else in the frame.",
    "battle_seastar_stand_0": "Square battle sprite showing the whole sea star. It stands facing the viewer with a bold red headband tied round its top arm, its two side arms raised in big red boxing gloves, its big eyes narrowed in a determined glare. Nothing else in the frame - no floor, no mat, no arena.",
    "battle_seastar_won_0": "Square battle portrait showing the whole sea star. It celebrates victory facing the viewer, one arm thrust up, wearing its red headband, a bold red medal on its chest. Nothing else in the frame.",
    "battle_seastar_lost_0": "Square battle portrait showing the sea star's whole body, not a close-up. Its face is as on the character sheet, except each eye is a black X. It has just lost a fight: it sits slumped with its arms drooping, tongue out, red headband askew, three bold red stars circling above. Nothing else in the frame.",
    "seastar_exercising_0": "Working out. In a bold red headband, the sea star does a star jump, all five arms flung wide, tongue out with effort. No mat, no floor.",
    "seastar_exercising_1": "Working out, second animation frame: the sea star lifts a chunky dumbbell overhead with both side arms, sweat drops flying.",
    "seastar_medicating_0": "Taking its medicine. The sea star eyes a big red pill warily, holding it at arm's length.",
    "seastar_medicating_1": "Taking its medicine, second animation frame: the sea star swallows the pill with a grimace and a red cross puffs above its head.",
    "seastar_drinking_0": "Drinking. The sea star tips a large bottle up with two arms and glugs from it, a few drops flying.",
    # The shorts-and-tape-measure version read as mush at 152x75; the before/after outline carries the joke in one shape.
    "seastar_ozempic_0": "Lost weight on Ozempic. The sea star stands proudly, now SLIM: its arms and middle clearly thinner than on the character sheet. It cheerfully holds up a big chunky red injection pen in one arm. Directly behind it, a DASHED black outline traces its old, much plumper star shape, so the before-and-after weight loss reads instantly. No shorts, no tape measure, no shells, no seaweed - nothing else in the frame.",
    "seastar_rehab_0": "In rehab. The sea star sits sheepishly on a small stool clutching a red pill bottle, looking guilty.",
    "seastar_rehab_1": "In rehab, second animation frame: the sea star throws the red pill bottle away with a determined look, sparkles around it.",
    # Kept innocent on purpose: a racier take tripped the image API's moderation on the badger.
    # The joke is the censor bar on a starfish: there is nothing to censor. One short
    # word only - the older 'TIP ME IN MUSSELS' sign was illegible at 152x75.
    "seastar_onlypets_0": "OnlyPets parody, the censor-bar joke. The sea star stands facing the viewer looking bashful, cheeks extra red, eyes glancing sideways. A thick SOLID BLACK rectangular censor bar runs straight across the middle of its body, and on the bar the single word PREMIUM is written in big blocky WHITE capital letters that fill the bar's height. (The joke: it is a starfish, there is nothing to censor.) Beside it a small camera on a tripod with a bold red recording light points at it. No other text. The censor bar is the ONE deliberate exception to the rule against large solid black areas below; nothing else is solid black. Keep it completely safe-for-work and wholesome: a cheeky, family-friendly parody of a social-media 'content creator', nothing sexual.",
}

SEASTAR_STRIPS: dict[str, dict] = {
    "seastar_tired": {
        "size": STRIP_SIZE,
        "targets": [
            "seastar_warning_tired_0",
            "seastar_warning_tired_1",
            "seastar_warning_tired_2",
        ],
        "subject": (
            "the sea star getting steadily sleepier - a three-step tired "
            "animation that loops. In ALL THREE frames the sea star stays "
            "STANDING UPRIGHT facing the viewer with all five arms visible, its "
            "big eyes and red cheek blush drawn exactly as on the character "
            "sheet, and its nightcap FILLED SOLID RED. It never lies down and "
            "never collapses into a heap"
        ),
        "panels": [
            "TOP PANEL - frame 1: the sea star yawns hugely, mouth wide open, one arm rubbing an eye, the solid red nightcap starting to slip down over its top arm.",
            "MIDDLE PANEL - frame 2: the same sea star, same size and same place in its panel, has let its top arm nod forward, eyes now half shut, the solid red nightcap slid further down.",
            "BOTTOM PANEL - frame 3: the same sea star, STILL STANDING UPRIGHT, same size and same place in its panel, top arm drooped forward with eyes fully closed, the solid red nightcap flopped over, and two Z's rising to one side. Draw the Z's as WHITE letters with a THICK BLACK OUTLINE around each one - never plain white letters with no outline.",
        ],
    },
}
# Frames that continue the previous frame of their animation: that frame's raw
# output is passed as a second reference so props carry over (the shell being
# eaten, the striped chair, the battery icon) instead of being redrawn fresh.
SEASTAR_CONTINUES: dict[str, str] = {
    "seastar_feeding_1": "seastar_feeding_0",
    "seastar_relaxing_2": "seastar_relaxing_1",
    "seastar_warning_drained_1": "seastar_warning_drained_0",
    # Sleeping follows tired on the badge, so the nightcap is carried down the
    # chain: tired_2 -> sleeping_0 -> sleeping_1 -> sleeping_2.
    "seastar_sleeping_0": "seastar_warning_tired_2",
    "seastar_sleeping_1": "seastar_sleeping_0",
    "seastar_sleeping_2": "seastar_sleeping_1",
}

# The generic CONTINUITY text tells the model to change the pose and treat image
# 2 as props only, which reads a worn hat as optional. These frames keep
# something the character WEARS, so they get a stricter continuity text.
WORN_CONTINUITY = (
    "IMAGE 2 is the PREVIOUS animation frame. The {pet} in this frame is still "
    "wearing exactly what it wears in image 2 - reproduce that worn item with "
    "the same shape, size, solid red fill, black outline and position on the "
    "body; it must not disappear, change colour or turn white. Image 2 also "
    "sets the line weight, scale and style to match. Draw the new moment the "
    "scene brief describes; do not return image 2 unchanged."
)

SEASTAR_CONTINUITY: dict[str, str] = {
    fid: WORN_CONTINUITY
    for fid in ("seastar_sleeping_0", "seastar_sleeping_1", "seastar_sleeping_2")
}


# ---------------------------------------------------------------------------
# Profiles for the pets whose art is already shipped (bartholomeus, cat, panda).
# They are only used for state frames, which are conditioned on the pet's own
# PCX frames, so there are no poses/strips/anchor here. "min_blob" is the
# smallest component the shipped art contains: despeckle must not eat it.
# ---------------------------------------------------------------------------
BARTHOLOMEUS_IDENTITY = (
    "The character is Bartholomeus, this game's hand-drawn scribble kid: a tall "
    "boxy white head drawn with thin sketchy black outline strokes and a few "
    "spiky black hair lines sticking out of the top and sides; two big square "
    "eyes that look like tiny screens, each with red scribble marks inside; a "
    "small narrow white body under the head with a bold RED pi-shaped symbol on "
    "its chest; two little feet, and a sprinkle of single black dots on the "
    "ground around them. Thin one-pixel lines, no filled black areas. Anyone "
    "must recognise it as the same character in every frame."
)
BARTHOLOMEUS_FREEDOM = (
    "Compose this frame yourself: choose Bartholomeus's posture, expression and "
    "the props that best tell the viewer what is happening, keeping its thin "
    "scribbly line weight. Be playful - this is a virtual-pet game and the "
    "scene should read instantly at a glance. Props are small and drawn with "
    "the same thin outlines."
)
CAT_IDENTITY = (
    "The character is this game's cat: a small SOLID BLACK silhouette cat "
    "sitting upright in profile-ish three-quarter view, with two tall pointed "
    "ears, two small white oval eyes with a black pupil dot, a compact body, "
    "short front legs, and a thin curled tail ending in a little hook on the "
    "right. There is no fur detail and no outline: the whole cat is one black "
    "shape with only the white eyes showing. Anyone must recognise it as the "
    "same cat in every frame."
)
CAT_FREEDOM = (
    "Compose this frame yourself: choose the cat's posture, expression and the "
    "props that best tell the viewer what is happening. The cat itself stays a "
    "solid black silhouette with white eyes; props around it are small white "
    "shapes with thin black outlines (and red where the brief asks), so they "
    "stand out against the black. Be playful and keep the cat's small size in "
    "the frame."
)
PANDA_IDENTITY = (
    "The character is this game's panda: a kawaii chibi giant panda sitting "
    "upright facing the viewer, with a big round white head, two round black "
    "ears, two big black oval eye patches each holding a white highlight dot, "
    "a small black nose and a tiny mouth, a white belly on a black body with "
    "black arms, and two black feet whose soles show bold RED paw pads. "
    "Chunky black outlines of an even thickness all round. Anyone must "
    "recognise it as the same panda in every frame."
)
PANDA_FREEDOM = (
    "Compose this frame yourself: choose the panda's posture, expression and "
    "the props that best tell the viewer what is happening. Panda-native props "
    "such as bamboo are welcome where they suit the scene. Be playful - this "
    "is a virtual-pet game and the scene should read instantly at a glance."
)

SLUG_IDENTITY = (
    "The character is this game's slug: a soft white garden slug seen from the side, "
    "with a rounded head hump on the left and a long low body that trails away to the "
    "right and ends in a flat-bottomed tail, all drawn with one thin continuous black "
    "outline and a plain white fill. Two long thin eyestalks rise from the head, each "
    "ending in a round eye with a black pupil dot and a tuft of little eyelash strokes. "
    "On the head hump are two small curved black marks and a gentle curved smile. Red is "
    "used only for small accents such as cheeks, hearts and props. Anyone must recognise "
    "it as the same slug in every frame."
)
SLUG_FREEDOM = (
    "Compose this frame yourself: choose the slug's posture, expression and the props "
    "that best tell the viewer what is happening, keeping its thin, even line weight. "
    "Be playful - this is a virtual-pet game and the scene should read instantly at a "
    "glance. Props are small and drawn with the same thin outlines."
)

NEW_PROFILES: dict[str, dict] = {
    "bartholomeus": {
        "noun": "Bartholomeus",
        "markings": "its thin sketchy outline strokes, hair lines, eyes and scribbles",
        "identity": BARTHOLOMEUS_IDENTITY,
        "freedom": BARTHOLOMEUS_FREEDOM,
        "poses": {},
        "strips": {},
        "min_blob": 1,
        "cfg": ART_PETS["bartholomeus"][1],
        "sheet": ART_PETS["bartholomeus"][2],
    },
    "cat": {
        "noun": "cat",
        "markings": "the cat's solid black body and tail (it is a black silhouette cat)",
        "identity": CAT_IDENTITY,
        "freedom": CAT_FREEDOM,
        "poses": {},
        "strips": {},
        "min_blob": 8,
        "cfg": ART_PETS["cat"][1],
        "sheet": ART_PETS["cat"][2],
    },
    "slug": {
        "noun": "slug",
        "markings": "its eyestalks, eyes with their eyelashes, the marks on its head and its outline",
        "identity": SLUG_IDENTITY,
        "freedom": SLUG_FREEDOM,
        "poses": {},
        "strips": {},
        "min_blob": 1,
        "cfg": ART_PETS["slug"][1],
        "sheet": ART_PETS["slug"][2],
    },
    "panda": {
        "noun": "panda",
        "markings": "the panda's ears, eye patches, nose, arms, legs and outlines",
        "identity": PANDA_IDENTITY,
        "freedom": PANDA_FREEDOM,
        "poses": {},
        "strips": {},
        "min_blob": 4,
        "cfg": ART_PETS["panda"][1],
        "sheet": ART_PETS["panda"][2],
    },
}


PROFILES: dict[str, dict] = {
    "badger": {
        "noun": "badger",
        "markings": "the badger's two head stripes, its ears, eyes and nose, and its black body coat",
        "identity": BADGER_IDENTITY,
        "freedom": BADGER_FREEDOM,
        "poses": BADGER_POSES,
        "strips": BADGER_STRIPS,
        "anchor_prompt": badger_anchor_prompt,
        "anchor_fidelity": "low",
        "continues": BADGER_CONTINUES,
    },
    "seastar": {
        "noun": "sea star",
        "markings": "its eyes, mouth and arm crease lines",
        "identity": SEASTAR_IDENTITY,
        "freedom": SEASTAR_FREEDOM,
        "poses": SEASTAR_POSES,
        "strips": SEASTAR_STRIPS,
        "anchor_prompt": seastar_anchor_prompt,
        "anchor_fidelity": "low",
        "continues": SEASTAR_CONTINUES,
        "continuity": SEASTAR_CONTINUITY,
    },
    **NEW_PROFILES,
}

if PET not in PROFILES:
    sys.exit(f"unknown BORNPETS_PET {PET!r}; known: {', '.join(PROFILES)}")
PROFILE = PROFILES[PET]
IDENTITY = PROFILE["identity"]
FREEDOM = PROFILE["freedom"]
POSES = PROFILE["poses"]
STRIPS = PROFILE["strips"]
CONTINUES: dict[str, str] = PROFILE.get("continues", {})
MIN_BLOB = PROFILE.get("min_blob", 12)

# The pet sheet is a SOURCE file that gets hand-edited between generations, so
# which one to write is a deliberate choice rather than a constant: point these at
# the working copy (v3, v4, ...) and the hand-edited original is never touched.
# A profile's own "sheet"/"cfg" win over the naming convention.
PET_SHEET = Path(os.environ.get("BORNPETS_SHEET", REPO / "assets" / PROFILE.get("sheet", f"bornpets-sprites-{PET}.png")))
PET_CFG = Path(os.environ.get("BORNPETS_CFG", REPO / "assets" / PROFILE.get("cfg", f"bornpets-sponsors-{PET}.json5")))

# gpt-image-2 treats a second reference as something to reproduce, so stated
# alone ("keep the props") it returns image 2 unchanged. The new moment has to
# be the explicit demand and the props the constraint.
CONTINUITY = (
    "IMAGE 2 is the PREVIOUS animation frame. It is given ONLY so you reuse the "
    "design of its props - their shapes, stripes, icons and red fills. This "
    "frame shows a LATER, DIFFERENT moment: redraw the character in the new "
    "pose and expression the scene brief describes, and change the props' state "
    "where the brief says so. A result that repeats image 2's pose or "
    "composition is wrong."
)


def build_prompt(frame_id: str) -> str:
    """Anchor fixes the character; the brief describes the action to stage.

    `input_fidelity=high` plus the anchor image is what keeps the badger's
    markings, proportions and line weight identical across 48 independent
    generations, while the scene itself is composed fresh each time.
    """
    continuity = ""
    if frame_id in CONTINUES:
        text = PROFILE.get("continuity", {}).get(frame_id, CONTINUITY)
        continuity = text.format(pet=PROFILE["noun"]) + "\n\n"
    return (
        "Draw one frame of a pixel-art virtual-pet game.\n\n"
        f"{IDENTITY}\n\n"
        f"{continuity}"
        f"Scene for this frame: {POSES[frame_id]}\n\n"
        f"{FREEDOM}\n\n"
        f"{style()}"
    )


# ---------------------------------------------------------------------------
# Frame bookkeeping
# ---------------------------------------------------------------------------


def frame_index() -> dict[str, dict]:
    """Map pet frame id -> geometry, plus the matching panda frame id.

    Only profiles with an `anchor_prompt` (the ones drawn from panda frames) are
    paired with the panda sheet; the shipped-art pets have their own row layout.
    """
    pet = load_sequences(PET_CFG)
    paired = "anchor_prompt" in PROFILE
    panda = load_sequences(PANDA_CFG) if paired else []
    if paired:
        assert len(pet) == len(panda), "configs must have identical row layout"

    out: dict[str, dict] = {}
    for n, bs in enumerate(pet):
        if bs.name.startswith("reserved"):
            continue
        ps = panda[n] if paired else None
        # Row identity is the position in the list, and frame size has to match
        # so a panda frame can be used as a reference. Frame COUNT may differ:
        # the badge derives it from the PCX filenames on flash, so the pet is
        # free to animate a row the panda draws as a single still.
        if ps is not None:
            assert (bs.frame_width, bs.frame_height) == (
                ps.frame_width,
                ps.frame_height,
            ), f"frame size mismatch: {bs.name} vs {ps.name}"
        for i in range(bs.frames):
            info = {"rect": bs.frame_rect(i)}
            if ps is not None and i < ps.frames:
                info["panda_rect"] = ps.frame_rect(i)
                info["panda_id"] = f"{ps.name}_{i}"
            out[f"{bs.name}_{i}"] = info
    return out


def panda_frame(rect: tuple[int, int, int, int]) -> Image.Image:
    x, y, w, h = rect
    return Image.open(PANDA_SHEET).convert("RGBA").crop((x, y, x + w, y + h))


def reference_png(frame: Image.Image, path: Path) -> tuple[int, int]:
    """Upscale a frame onto the API canvas that best matches its aspect."""
    cw, ch = gen_size(frame.width, frame.height)
    scale = min(cw // frame.width, ch // frame.height)
    up = frame.resize((frame.width * scale, frame.height * scale), Image.NEAREST)
    # Magenta, not transparency: showing the model the same chroma key it is
    # asked to produce stops it from rendering "black" as a hole in the alpha.
    canvas = Image.new("RGBA", (cw, ch), KEY)
    canvas.paste(up, ((cw - up.width) // 2, (ch - up.height) // 2), up)
    canvas.save(path)
    return (cw, ch)


# ---------------------------------------------------------------------------
# Generation
# ---------------------------------------------------------------------------


def _checked(key: str, source: str) -> str:
    """A key that would corrupt the curl config (quote, newline, ...) is refused."""
    if not re.fullmatch(r"[A-Za-z0-9_\-]+", key):
        sys.exit(f"{source} does not hold a plain API key (unexpected characters)")
    return key


def _read_key_file(path: Path) -> str:
    """Read a key file, refusing one in the repo or readable by others.

    Messages name the path, never the content.
    """
    try:
        resolved = path.resolve(strict=True)
    except OSError:
        sys.exit(f"key file {path} does not exist")
    if resolved.is_relative_to(REPO.resolve()):
        sys.exit(f"key file {path} is inside the repository; keep it outside")
    if resolved.stat().st_mode & 0o077:
        sys.exit(f"key file {path} is accessible by others; run chmod 600 on it")
    if resolved.stat().st_nlink > 1:
        sys.exit(f"key file {path} has several hard links; one of them may be in the repository")
    key = resolved.read_text(encoding="utf-8").strip()
    if not key:
        sys.exit(f"key file {path} is empty")
    return _checked(key, f"key file {path}")


def api_key(key_file: str | None = None) -> str:
    """The API key: the argument, KEY_FILE (--key-file), OPENAI_API_KEY_FILE,
    OPENAI_API_KEY, then ~/.config/openai/api-key, in that order.

    An `export` in an interactive shell does not reach a tool that was started
    before it, so the file sources are what make the key reachable without
    putting it on a command line (argv is world-readable via /proc).
    """
    explicit = key_file or KEY_FILE or os.environ.get("OPENAI_API_KEY_FILE")
    if explicit:
        return _read_key_file(Path(explicit))
    key = os.environ.get("OPENAI_API_KEY", "").strip()
    if key:
        return _checked(key, "OPENAI_API_KEY")
    default = Path.home() / ".config/openai/api-key"
    if default.is_file():
        return _read_key_file(default)
    sys.exit(
        "no API key: use --key-file, set OPENAI_API_KEY_FILE or OPENAI_API_KEY, "
        f"or put the key in {default} (chmod 600)"
    )


def call_api(
    refs: list[Path],
    prompt: str,
    quality: str,
    size: tuple[int, int],
    fidelity: str = "high",
) -> bytes:
    key = api_key()

    # The Authorization header goes in via `-K -` (a curl config read from
    # stdin), never as an argv element: anything on the command line is world
    # readable in /proc and shows up in `ps` output for every local user.
    cmd = [
        "curl", "-sS", "-K", "-", API_URL,
        "-F", f"model={MODEL}",
    ]
    for ref in refs:
        cmd += ["-F", f"image[]=@{ref};type=image/png"]
    cmd += [
        # --form-string, not -F: curl parses `;` in a -F value as a parameter
        # separator and silently drops everything after it, so a prompt with a
        # semicolon would reach the API cut short.
        "--form-string", f"prompt={prompt}",
        "-F", f"size={size[0]}x{size[1]}",
        "-F", "output_format=png",
        "-F", f"quality={quality}",
        "-F", "n=1",
    ]
    # Keeps the model reproducing the reference character instead of
    # reinterpreting it — the main defence against drift between frames.
    # It also pins the anchor's framing, so a frame that needs a different
    # camera distance has to relax it (see FIDELITY_OVERRIDES). Only the
    # gpt-image-1 family takes it; later models always use the references at
    # full fidelity and reject the parameter.
    if MODEL.startswith("gpt-image-1"):
        cmd += ["-F", f"input_fidelity={fidelity}"]
    curl_cfg = f'header = "Authorization: Bearer {key}"\n'
    # curl needs the key on stdin only, so keep it out of its environment too.
    env = {k: v for k, v in os.environ.items() if k != "OPENAI_API_KEY"}
    res = subprocess.run(cmd, input=curl_cfg.encode(), capture_output=True, env=env)
    if res.returncode != 0:
        raise RuntimeError(f"curl failed ({res.returncode}): {res.stderr.decode()[:400]}")
    payload = json.loads(res.stdout)
    if "data" not in payload:
        raise RuntimeError(f"API error: {json.dumps(payload)[:600]}")
    return base64.b64decode(payload["data"][0]["b64_json"])


def cmd_anchor(args: argparse.Namespace) -> None:
    """Generate the character sheet every other frame is conditioned on."""
    idx = frame_index()
    work = Path(args.work)
    (work / "ref").mkdir(parents=True, exist_ok=True)
    ref_path = work / "ref" / f"{ANCHOR_ID}.png"
    size = reference_png(panda_frame(idx[ANCHOR_ID]["panda_rect"]), ref_path)
    # Extra character references (e.g. badger drawings) follow the panda style
    # frame; the anchor prompt says how to use them.
    refs = [ref_path] + [Path(r) for r in args.ref]
    print(f"{ANCHOR_ID}: generating anchor...", flush=True)
    anchor_path(work).write_bytes(
        call_api(
            refs,
            PROFILE["anchor_prompt"](),
            args.quality,
            size,
            PROFILE["anchor_fidelity"],
        )
    )
    print(f"wrote {anchor_path(work)}")


def cmd_gen(args: argparse.Namespace) -> None:
    idx = frame_index()
    # Rows declare spare frame slots; only the ones with a brief get drawn.
    targets = [t for t in select(idx, args) if args.only or t in POSES]
    work = Path(args.work)
    (work / "ref").mkdir(parents=True, exist_ok=True)
    (work / "raw").mkdir(parents=True, exist_ok=True)

    anchor = anchor_path(work)
    if not anchor.exists():
        sys.exit(f"no anchor at {anchor} — run the `anchor` subcommand first")

    failed: list[str] = []
    for n, fid in enumerate(targets, 1):
        raw_path = work / "raw" / f"{fid}.png"
        if raw_path.exists() and not args.force:
            print(f"[{n}/{len(targets)}] {fid}: cached")
            continue
        # Anchor only. Feeding the matching panda frame in as a second
        # reference made the model trace it, and the badger came out a panda
        # clone; the scene brief in the prompt carries the action instead.
        _, _, fw, fh = idx[fid]["rect"]
        print(f"[{n}/{len(targets)}] {fid}: generating...", flush=True)
        try:
            refs = [anchor]
            if fid in CONTINUES:
                prev = work / "raw" / f"{CONTINUES[fid]}.png"
                if not prev.exists():
                    raise RuntimeError(f"needs {prev.name} generated first")
                refs.append(prev)
            raw_path.write_bytes(
                call_api(
                    refs,
                    build_prompt(fid),
                    args.quality,
                    gen_size(fw, fh),
                    FIDELITY_OVERRIDES.get(fid, "high"),
                )
            )
        except RuntimeError as exc:
            # One rejected or failed frame must not abandon the other 47; the
            # run reports what is missing at the end so it can be retried.
            failed.append(fid)
            print(f"[{n}/{len(targets)}] {fid}: FAILED - {exc}", flush=True)

    if failed:
        print(f"\n{len(failed)} frame(s) failed: {', '.join(failed)}")


# ---------------------------------------------------------------------------
# Quantisation
# ---------------------------------------------------------------------------


def classify(px: tuple[int, int, int, int]) -> tuple[int, int, int, int]:
    """Map an RGBA pixel onto the badge palette (see template.rs::sample_frame)."""
    r, g, b, a = px
    if a < 128:
        return TRANSPARENT
    if r > 140 and g < 110 and b < 110:
        return RED
    return WHITE if (r * 299 + g * 587 + b * 114) // 1000 >= 128 else BLACK


def despeckle(img: Image.Image, min_blob: int = 12) -> Image.Image:
    """Drop opaque islands smaller than `min_blob` pixels.

    The model reliably sneaks a faint shadow or ground line along the bottom
    edge despite being told not to, and downscaling scatters it into crumbs.
    Deleting whole small connected components clears that, where a
    single-pixel filter left the crumbs behind.
    """
    w, h = img.size
    px = img.load()
    out = img.copy()
    op = out.load()
    seen = [[False] * w for _ in range(h)]

    for sy in range(h):
        for sx in range(w):
            if seen[sy][sx] or px[sx, sy][3] < 128:
                continue
            blob, stack = [], [(sx, sy)]
            seen[sy][sx] = True
            while stack:
                x, y = stack.pop()
                blob.append((x, y))
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if (
                        0 <= nx < w
                        and 0 <= ny < h
                        and not seen[ny][nx]
                        and px[nx, ny][3] >= 128
                    ):
                        seen[ny][nx] = True
                        stack.append((nx, ny))
            if len(blob) < min_blob:
                for x, y in blob:
                    op[x, y] = TRANSPARENT
    return out


def dekey(img: Image.Image) -> Image.Image:
    """Punch the magenta chroma key out to transparency."""
    out = img.copy()
    px = out.load()
    w, h = out.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a < 128 or (r > 120 and b > 120 and g < 110 and abs(r - b) < 90):
                px[x, y] = TRANSPARENT
    return out


def quantize_one(raw: Image.Image, size: tuple[int, int]) -> Image.Image:
    """Scale the raw AI art down to one badge frame and palettise it."""
    raw = dekey(raw)
    bbox = raw.getbbox()
    if bbox is None:
        raise ValueError("AI output is fully transparent")
    art = raw.crop(bbox)

    # The model stages the whole frame now, so scale its art to the frame
    # itself rather than to whatever bounding box the panda happened to use.
    scale = min(size[0] / art.width, size[1] / art.height)
    art = ink_resize(art, (max(1, round(art.width * scale)), max(1, round(art.height * scale))))

    out = Image.new("RGBA", size, TRANSPARENT)
    out.paste(art, ((size[0] - art.width) // 2, (size[1] - art.height) // 2))
    return despeckle(out)


def detect_bands(img: Image.Image, n: int) -> list[tuple[int, int]]:
    """Find the `n` panel bands of a strip by looking for empty rows.

    The model does not honour exact thirds — it centres each panel on its own
    art, so a blind slice at n/3 saws a panel's feet off and hands them to the
    next band. Cutting in the middle of the blank gaps between panels tracks
    wherever the model actually put them. Falls back to equal bands if the gaps
    cannot be found, which is what a panel bleeding into its neighbour looks
    like.
    """
    keyed = dekey(img)
    px = keyed.load()
    blank = [
        all(px[x, y][3] < 128 for x in range(keyed.width))
        for y in range(keyed.height)
    ]

    gaps: list[tuple[int, int]] = []
    start = None
    for y, is_blank in enumerate(blank):
        if is_blank and start is None:
            start = y
        elif not is_blank and start is not None:
            gaps.append((start, y - 1))
            start = None
    if start is not None:
        gaps.append((start, keyed.height - 1))

    # Interior gaps only: leading and trailing blank space is margin, not a cut.
    interior = [g for g in gaps if g[0] > 0 and g[1] < keyed.height - 1]
    interior.sort(key=lambda g: g[1] - g[0], reverse=True)
    cuts = sorted((g[0] + g[1]) // 2 for g in interior[: n - 1])

    if len(cuts) != n - 1:
        band = keyed.height // n
        print(f"  warning: found {len(cuts)} gap(s), expected {n - 1}; using equal bands")
        cuts = [band * (i + 1) for i in range(n - 1)]

    edges = [0, *cuts, keyed.height]
    return [(edges[i], edges[i + 1]) for i in range(n)]


def outline_white_blobs(img: Image.Image, min_blob: int = 6) -> Image.Image:
    """Give a black outline to any shape drawn in white alone.

    Transparent pixels are skipped by the badge (sprite_loader.rs), so a sprite
    sits on bare white paper: an all-white shape with no black in it has nothing
    to read against and simply disappears. The model does this to sleep Z's and
    the odd speech bubble. The panda's own Z's are white with a black outline,
    so this restores what the artwork should have had.
    """
    w, h = img.size
    px = img.load()
    out = img.copy()
    op = out.load()
    seen = [[False] * w for _ in range(h)]

    for sy in range(h):
        for sx in range(w):
            if seen[sy][sx] or px[sx, sy][3] < 128:
                continue
            blob, stack, has_black = [], [(sx, sy)], False
            seen[sy][sx] = True
            while stack:
                x, y = stack.pop()
                blob.append((x, y))
                if px[x, y][:3] == BLACK[:3]:
                    has_black = True
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < w and 0 <= ny < h and not seen[ny][nx] and px[nx, ny][3] >= 128:
                        seen[ny][nx] = True
                        stack.append((nx, ny))
            if has_black or len(blob) < min_blob:
                continue
            for x, y in blob:
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < w and 0 <= ny < h and px[nx, ny][3] < 128:
                        op[nx, ny] = BLACK
    return out


def quantize_panels(
    panels: list[Image.Image], size: tuple[int, int]
) -> list[Image.Image]:
    """Palettise strip panels through one shared crop and scale.

    Quantising each panel on its own bbox gives each its own scale factor, so
    the badger visibly changes size from frame to frame. Sharing the union bbox
    and one scale keeps it still and lets only the animation move.
    """
    keyed = [dekey(p) for p in panels]
    boxes = [k.getbbox() for k in keyed]
    if all(b is None for b in boxes):
        raise ValueError("every panel is empty")

    live = [b for b in boxes if b is not None]
    union = (
        min(b[0] for b in live),
        min(b[1] for b in live),
        max(b[2] for b in live),
        max(b[3] for b in live),
    )
    uw, uh = union[2] - union[0], union[3] - union[1]
    scale = min(size[0] / uw, size[1] / uh)
    tw, th = max(1, round(uw * scale)), max(1, round(uh * scale))
    ox, oy = (size[0] - tw) // 2, (size[1] - th) // 2

    out = []
    for k in keyed:
        art = k.crop(union).resize((tw, th), Image.LANCZOS)
        frame = Image.new("RGBA", size, TRANSPARENT)
        frame.paste(art, (ox, oy))
        px = frame.load()
        for y in range(size[1]):
            for x in range(size[0]):
                px[x, y] = classify(px[x, y])
        out.append(outline_white_blobs(despeckle(frame)))
    return out


def palettise(img: Image.Image) -> Image.Image:
    """Snap a hand-drawn frame onto the badge palette, at its native size.

    Hand edits come back with a magenta backdrop and slightly-off reds (a paint
    program's #E10021 rather than #FF0000), which the badge's sampler would
    misread. No rescale and no despeckle: deliberate one-pixel details are the
    whole point of a hand fix.
    """
    out = dekey(img)
    px = out.load()
    for y in range(out.height):
        for x in range(out.width):
            px[x, y] = classify(px[x, y])
    return out


def cmd_strip(args: argparse.Namespace) -> None:
    """Generate one multi-panel image, then cut it into individual frames."""
    key = args.strip
    if key not in STRIPS:
        sys.exit(f"unknown strip {key!r}; known: {', '.join(STRIPS)}")
    spec = STRIPS[key]
    n = len(spec["panels"])

    idx = frame_index()
    targets = spec["targets"]
    if len(targets) != n:
        sys.exit(f"{key}: {n} panels but {len(targets)} targets")
    missing = [t for t in targets if t not in idx]
    if missing:
        sys.exit(f"{key}: targets not in the config: {missing}")

    work = Path(args.work)
    for d in ("raw", "strips", "frames"):
        (work / d).mkdir(parents=True, exist_ok=True)

    anchor = anchor_path(work)
    if not anchor.exists():
        sys.exit(f"no anchor at {anchor} — run the `anchor` subcommand first")

    strip_raw = work / "strips" / f"{key}.strip.png"
    if args.reslice:
        if not strip_raw.exists():
            sys.exit(f"--reslice needs an existing strip at {strip_raw}")
        print(f"{key}: re-slicing {strip_raw} (no API call)")
    elif strip_raw.exists() and not args.force:
        print(f"{key}: cached strip at {strip_raw}")
    else:
        print(f"{key}: generating {n}-panel strip...", flush=True)
        strip_raw.write_bytes(
            call_api([anchor], strip_prompt(key), args.quality, spec["size"])
        )
        print(f"wrote {strip_raw}")

    img = Image.open(strip_raw).convert("RGBA")
    bands = detect_bands(img, n)
    panels = [img.crop((0, top, img.width, bot)) for top, bot in bands]

    # Each band becomes that frame's raw art, so a strip panel can be inspected
    # and re-quantised exactly like a singly-generated frame.
    for target, panel in zip(targets, panels):
        panel.save(work / "raw" / f"{target}.png")

    rect = idx[targets[0]]["rect"]
    frames = quantize_panels(panels, (rect[2], rect[3]))
    for i, (target, frame) in enumerate(zip(targets, frames)):
        frame.save(work / "frames" / f"{target}.png")
        top, bot = bands[i]
        print(f"  panel {i} (rows {top}-{bot}) -> {target}")


def cmd_quantize(args: argparse.Namespace) -> None:
    idx = frame_index()
    work = Path(args.work)
    (work / "frames").mkdir(parents=True, exist_ok=True)
    for fid in select(idx, args):
        rect = idx[fid]["rect"]
        size = (rect[2], rect[3])

        # A hand-fixed frame always wins over the generated one, and survives
        # re-running gen/quantize.
        override = work / "overrides" / f"{fid}.png"
        if override.exists():
            art = Image.open(override).convert("RGBA")
            if art.size != size:
                sys.exit(f"{fid}: override is {art.size}, expected {size}")
            palettise(art).save(work / "frames" / f"{fid}.png")
            print(f"{fid}: override")
            continue

        raw_path = work / "raw" / f"{fid}.png"
        if not raw_path.exists():
            print(f"{fid}: no raw output, skipping")
            continue
        art = quantize_one(Image.open(raw_path).convert("RGBA"), size)
        art.save(work / "frames" / f"{fid}.png")
        print(f"{fid}: quantized")


# ---------------------------------------------------------------------------
# State frames: the pet's own shipped frames are the references
# ---------------------------------------------------------------------------

REF_SCALE = 6
CANVAS = (1536, 1024)
REF_ORIGIN = ((CANVAS[0] - FRAME[0] * REF_SCALE) // 2, (CANVAS[1] - FRAME[1] * REF_SCALE) // 2)
_PCX_COLOURS = {0: BLACK, 1: RED, 2: WHITE}

# Anim ids after idle (0x01) whose frame 0 is sent as a reference (design 3.4).
STATE_REFS: dict[str, tuple[int, ...]] = {
    "tipsy": (0x25, 0x02),
    "wasted": (0x25, 0x0B, 0x04),
    "hung_over": (0x06, 0x07, 0x25),
    "sore": (0x03, 0x23, 0x07),
    "injured": (0x03, 0x0D, 0x23),
    "groggy": (0x0A, 0x08, 0x10),
    "stuffed": (0x0C, 0x0B),
    "thirsty": (0x09, 0x25),
    "wired": (0x0F, 0x02, 0x23),
    "throwing_up": (0x03, 0x0C, 0x23),
    "needs_rehab": (0x27, 0x04, 0x0B),   # rehab, critical tired, miserable
    "starving": (0x05, 0x09, 0x0C),      # critical hungry, warning hungry, feeding
    "washing": (0x0E, 0x0F),             # relaxing, playing
}

STATE_BRIEFS: dict[str, str] = {
    "tipsy": "Tipsy. The {pet} sways happily with a small glass, a silly grin, one little bubble floating up. Nothing else in the frame.",
    "wasted": "Wasted. The {pet} slumps against nothing, swirly eyes, a hiccup bubble, an empty bottle lying on its side. Nothing else in the frame.",
    "hung_over": "Hung over. The {pet} squints in pain under a red ice pack, a glass of water in its paw, a tiny storm cloud over its head. Nothing else in the frame.",
    "sore": "Sore. The {pet} winces and rubs one aching shoulder, two small red ache marks beside it. Nothing else in the frame.",
    "injured": "Injured. The {pet} stands on one leg with the other in a white bandage and a red cross plaster, leaning on a crutch. Nothing else in the frame.",
    "groggy": "Groggy. Overslept: bleary half-open eyes, messy bed hair, a big yawn, clutching a coffee mug. Nothing else in the frame.",
    "stuffed": "Stuffed. The {pet} leans back with a round, overfull belly, an empty plate beside it and a small burp puff. Nothing else in the frame.",
    "thirsty": "Thirsty. The {pet} pants with its tongue out beside an empty glass, one sweat drop on its brow. Nothing else in the frame.",
    "wired": "Wired. Overexcited: wide eyes, fur on end, jittery motion lines, a red energy-drink can in its paw. Nothing else in the frame.",
    "throwing_up": "Throwing up after sport on a full belly. The {pet} bends over with dizzy eyes and a cartoon splash of sick drawn in WHITE with a thick BLACK outline in front of it, a few wobbly lines. No red anywhere in the sick. Nothing else in the frame.",
    "needs_rehab": "Needs rehab after far too much booze and pills. The {pet} is shaky and sweating, holding on with both arms to a lamp post to keep itself standing, bleary red-rimmed eyes, a few tremble lines; an empty bottle and a spilled strip of pills lie at its feet. It is the same young {pet} as in its idle pose, NOT old: no walking frame, no cane, no grey hair, no wrinkles. Clearly worse than its idle pose. Nothing else in the frame.",
    "starving": "Starving. The {pet} looks starved: visibly thinner and droopier than its idle pose, sunken eyes and cheeks, holding out an empty bowl, a small rumble mark by its belly. Clearly different from its idle pose. Nothing else in the frame.",
    "washing": "Washing. The {pet} sits in a small round wash tub and scrubs itself with a little brush, a few round soap bubbles around it and one on its head, looking content. Keep the tub small and simple, with no shells, plants, towels or other props. Clearly different from its idle pose. Nothing else in the frame.",
}

# Every state prompt carries this, after style() and before any owner note (amendment 1):
# the 78 frames have to fit the volume, and a frame over 2048 B costs a second cluster.
FLAT_FILL = (
    "FLAT FILLS, SMALL FILE: large areas of one flat colour; no dithering, no hatching, no "
    "texture, no stippling, no speckles or scattered dots; few small details; fur, stripes and shading are a few big solid shapes, never many small "
    "marks; every outline a continuous unbroken line. Solid white stays solid white."
)


def pet_prefix() -> str:
    return ART_PETS[PET][0]


def state_frame_ids() -> list[str]:
    return [f"{ART_PETS[PET][3]}_{s}_0" for s in STATES]


def key_canvas(rows_w: int, rows_h: int, rows: list[list[int]], scale: int,
               origin: tuple[int, int]) -> Image.Image:
    """A decode_pcx result as RGBA on a magenta KEY CANVAS, nearest-upscaled."""
    small = Image.new("RGBA", (rows_w, rows_h), KEY)
    px = small.load()
    for y, row in enumerate(rows):
        for x, v in enumerate(row):
            if v in _PCX_COLOURS:
                px[x, y] = _PCX_COLOURS[v]
    canvas = Image.new("RGBA", CANVAS, KEY)
    canvas.paste(small.resize((rows_w * scale, rows_h * scale), Image.NEAREST), origin)
    return canvas


_INDEX = {BLACK: 0, RED: 1, WHITE: 2, TRANSPARENT: 3}


def ref_rows(prefix: str, aa: int) -> list[list[int]]:
    """Frame 0 of anim row `aa` as badge indices (0 black, 1 red, 2 white, 3 transparent).

    The shipped PCX when assets/to-badge has it; otherwise the same cell cropped from the
    pet's sheet (the retired Drained rows keep their cells there) and mapped through classify.
    """
    pcx = TO_BADGE / f"{prefix}{aa:02X}00.PCX"
    if pcx.is_file():
        return decode_pcx(pcx)[2]
    _pet, (_pp, cfg, sheet, _seq) = next((k, v) for k, v in ART_PETS.items() if v[0] == prefix)
    seq = load_sequences(REPO / "assets" / cfg)[aa]
    x, y, w, h = seq.frame_rect(0)
    cell = Image.open(REPO / "assets" / sheet).convert("RGBA").crop((x, y, x + w, y + h))
    px = cell.load()
    return [[_INDEX[classify(px[cx, cy])] for cx in range(w)] for cy in range(h)]


def ref_png(prefix: str, aa: int, path: Path) -> None:
    """A shipped frame at a fixed scale in a fixed rectangle of the API canvas."""
    rows = ref_rows(prefix, aa)
    key_canvas(len(rows[0]), len(rows), rows, REF_SCALE, REF_ORIGIN).save(path)


def state_refs(prefix: str, state: str, work: Path) -> list[Path]:
    """Idle first (it is the primary image), then the state's table frames."""
    (work / "ref").mkdir(parents=True, exist_ok=True)
    out = []
    for aa in (0x01, *STATE_REFS[state]):
        path = work / "ref" / f"{prefix}{aa:02X}00.png"
        ref_png(prefix, aa, path)
        out.append(path)
    return out


def state_prompt(state: str, note: str | None = None) -> str:
    n = len(STATE_REFS[state]) + 1
    pet = PROFILE["noun"]
    text = (
        "Draw one frame of a pixel-art virtual-pet game.\n\n"
        f"{IDENTITY}\n\n"
        f"IMAGE 1 is this {pet} in its idle pose: same character, same outline "
        "thickness all round, same size and the same place inside the same "
        f"rectangle. IMAGES 2 to {n} are other frames of the same pet: match "
        "their line weight and style, not their poses.\n\n"
        f"{STATE_BRIEFS[state].format(pet=pet)}\n\n"
        f"{FREEDOM}\n\n"
        f"{style()}\n\n"
        f"{FLAT_FILL}"
    )
    if note:
        text += f"\n\nOwner feedback on the previous attempt: {note}"
    return text


_MARK_MAX = 12  # opaque islands this small (the cat's are 3x3) are corner marks, not the pet


def rows_box(rows: list[list[int]], mark_max: int = _MARK_MAX) -> tuple[int, int, int, int]:
    """Bounding box (x0, y0, x1, y1, exclusive) of the opaque art in badge rows.

    Islands of at most `mark_max` pixels are left out: the Bartholomeus and Cat
    idle frames carry tiny marks in their top corners, which would otherwise
    stretch the box to the whole frame and blow the pet up when it is cut.
    """
    h, w = len(rows), len(rows[0])
    seen = [[False] * w for _ in range(h)]
    xs, ys = [], []
    for sy in range(h):
        for sx in range(w):
            if seen[sy][sx] or rows[sy][sx] == 3:
                continue
            island, stack = [], [(sx, sy)]
            seen[sy][sx] = True
            while stack:
                x, y = stack.pop()
                island.append((x, y))
                for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
                    if 0 <= nx < w and 0 <= ny < h and not seen[ny][nx] and rows[ny][nx] != 3:
                        seen[ny][nx] = True
                        stack.append((nx, ny))
            if len(island) > mark_max:
                xs += [p[0] for p in island]
                ys += [p[1] for p in island]
    return min(xs), min(ys), max(xs) + 1, max(ys) + 1


def idle_box(prefix: str) -> tuple[int, int, int, int]:
    """Bounding box (x0, y0, x1, y1, exclusive) of the pet's own shipped idle frame."""
    return rows_box(decode_pcx(TO_BADGE / f"{prefix}0100.PCX")[2])


_HALO_MIN = 24    # a blend darker than this is the pet's own black outline, not halo


def dekey_halo(img: Image.Image) -> Image.Image:
    """`dekey`, plus the soft shadow halo the model paints around the pet.

    The halo fades from black out into the key colour, so the plain key test
    leaves its dark inner part opaque: it grows the bbox and survives as black
    specks. Flood in from the border through every purple-ish pixel that is
    not near-black (outline); the pet's outline (near black) and red/white art stop it.
    """
    out = dekey(img)
    w, h = out.size
    px = out.load()

    def halo(p) -> bool:
        r, g, b, a = p
        return a < 128 or (g < 110 and abs(r - b) < 60 and max(r, b) >= _HALO_MIN)

    seen = [[False] * w for _ in range(h)]
    stack = [(x, y) for x in range(w) for y in (0, h - 1)] + [(x, y) for y in range(h) for x in (0, w - 1)]
    while stack:
        x, y = stack.pop()
        if not (0 <= x < w and 0 <= y < h) or seen[y][x]:
            continue
        seen[y][x] = True
        if halo(px[x, y]):
            px[x, y] = TRANSPARENT
            stack += [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
    return out


INK_SOLID = 0.55     # a badge pixel more than this share black is black outright
INK_RIDGE_MIN = 0.2  # a thinner line still claims the darkest pixel across it
INK_EDGE_MIN = 0.05  # any outline at all in a silhouette pixel makes it black


def ink_resize(img: Image.Image, size: tuple[int, int]) -> Image.Image:
    """Shrink art onto the badge palette without losing thin black lines.

    A filter resize averages a thin black line with the white beside it into
    grey, which `classify` reads as white: the outline breaks up. Instead,
    classify the full-size pixels first, then take the exact area share of each
    ink per badge pixel (BOX resize of each ink mask). A pixel is black when
    black covers more than INK_SOLID of it, or when it is the darkest pixel
    across a line (a ridge in the black share, ties going to the right/lower
    pixel so a line stays one pixel wide) with at least INK_RIDGE_MIN black.
    The rest is transparent when under half covered, else red or white by
    majority. Finally a silhouette pixel (next to transparency) with any
    outline in it (INK_EDGE_MIN) is black, so the outer outline stays closed.
    """
    import numpy as np

    a = np.asarray(img.convert("RGBA"), dtype=np.int32)
    r, g, b, al = a[..., 0], a[..., 1], a[..., 2], a[..., 3]
    opaque = al >= 128
    red = opaque & (r > 140) & (g < 110) & (b < 110)
    white = opaque & ~red & ((r * 299 + g * 587 + b * 114) // 1000 >= 128)
    black = opaque & ~red & ~white

    def share(mask):
        m = Image.fromarray(mask.astype(np.float32), "F").resize(size, Image.BOX)
        return np.asarray(m, dtype=np.float32)

    cb, cr, cw = share(black), share(red), share(white)
    p = np.pad(cb, 1, mode="edge")
    left, right, up, down = p[1:-1, :-2], p[1:-1, 2:], p[:-2, 1:-1], p[2:, 1:-1]
    e = 1e-4   # BOX shares carry float noise; equal must compare equal
    ridge = ((cb >= left - e) & (cb > right + e)) | ((cb >= up - e) & (cb > down + e))
    is_black = (cb > INK_SOLID) | ((cb >= INK_RIDGE_MIN) & ridge)
    clear = ~is_black & (cb + cr + cw < 0.5)
    t = np.pad(clear, 1, constant_values=True)
    edge = ~clear & (t[1:-1, :-2] | t[1:-1, 2:] | t[:-2, 1:-1] | t[2:, 1:-1])
    is_black |= edge & (cb >= INK_EDGE_MIN)
    is_red = ~is_black & ~clear & (cr > cw)

    out = np.empty((size[1], size[0], 4), dtype=np.uint8)
    out[...] = WHITE
    out[is_red] = RED
    out[is_black] = BLACK
    out[clear] = TRANSPARENT
    return Image.fromarray(out, "RGBA")


def close_outline(img: Image.Image) -> Image.Image:
    """Blacken every white pixel that touches transparency: close a broken outline.

    For finished pixel art that has no raw to re-cut (hand-drawn sheet frames).
    Judged on the input, so only the outermost white ring changes; red art and
    the frame edge (art cut off there on purpose) are left alone.
    """
    out = img.convert("RGBA").copy()
    src = img.convert("RGBA").load()
    px = out.load()
    w, h = out.size
    for y in range(h):
        for x in range(w):
            if src[x, y] == WHITE and any(
                    0 <= x + dx < w and 0 <= y + dy < h and src[x + dx, y + dy][3] < 128
                    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
                px[x, y] = BLACK
    return out


def cut_pet(raw: Image.Image, idle: tuple[int, int, int, int], margin: int = 4) -> Image.Image:
    """Cut a whole-pet raw to a FRAME that is framed like the pet's idle frame.

    The model draws the whole pet bigger than the hand-drawn art, and the
    firmware lays toast, HAX counter, status line and litter over the sprite.
    So take the bbox of the art, scale it uniformly to fit (frame width - 2 *
    margin) x idle height, and put it bottom-aligned at the idle bottom, centred
    on the idle centre, clamped inside the frame. A raw with no art is blank.
    """
    raw = dekey_halo(raw.convert("RGBA"))
    out = Image.new("RGBA", FRAME, TRANSPARENT)
    bbox = raw.getbbox()
    if bbox is None:
        return out
    art = raw.crop(bbox)
    scale = min((FRAME[0] - 2 * margin) / art.width, (idle[3] - idle[1]) / art.height)
    art = ink_resize(art, (max(1, round(art.width * scale)), max(1, round(art.height * scale))))
    x = min(max((idle[0] + idle[2] - art.width) // 2, margin), FRAME[0] - margin - art.width)
    out.paste(art, (x, idle[3] - art.height))
    return despeckle(out, MIN_BLOB)


_N8 = ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1))


def merge_islands(img: Image.Image, min_island: int) -> Image.Image:
    """Recolour small one-colour islands enclosed by exactly one other colour.

    Islands and their surroundings use 8-connectivity, so a diagonal 1-px line
    is one component and is not mistaken for specks. Repeats until nothing
    changes. A component with a transparent neighbour or on the frame edge is
    never changed, so silhouette and outline stay. Within a pass a component
    that touches a pixel recoloured in that pass waits for the next pass, so
    sizes are never stale.
    """
    out = img.convert("RGBA").copy()
    w, h = out.size
    px = out.load()
    changed = True
    while changed:
        changed = False
        seen = [[False] * w for _ in range(h)]
        dirty: set[tuple[int, int]] = set()
        for sy in range(h):
            for sx in range(w):
                if seen[sy][sx] or px[sx, sy][3] < 128:
                    continue
                colour = px[sx, sy]
                comp, stack, edge = [], [(sx, sy)], False
                seen[sy][sx] = True
                around: set = set()
                touched: set[tuple[int, int]] = set()
                while stack:
                    x, y = stack.pop()
                    comp.append((x, y))
                    for dx, dy in _N8:
                        nx, ny = x + dx, y + dy
                        if not (0 <= nx < w and 0 <= ny < h):
                            edge = True
                        elif px[nx, ny] == colour:
                            if not seen[ny][nx]:
                                seen[ny][nx] = True
                                stack.append((nx, ny))
                        else:
                            touched.add((nx, ny))
                            around.add(px[nx, ny] if px[nx, ny][3] >= 128 else TRANSPARENT)
                if len(comp) >= min_island or edge or len(around) != 1:
                    continue
                (other,) = around
                if other == TRANSPARENT:
                    continue
                if dirty and (dirty & touched or dirty.intersection(comp)):
                    changed = True      # look again next pass
                    continue
                for x, y in comp:
                    px[x, y] = other
                dirty.update(comp)
                changed = True
    return out


def _is_ok(value: object) -> bool:
    return not isinstance(value, str) or value.strip().rstrip(".").casefold() in ("", "ok")


def load_notes(path: str | Path) -> dict:
    """The --note-file JSON object; a missing or malformed file is a clean exit."""
    try:
        notes = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        sys.exit(f"--note-file {path} is not readable JSON: {exc}")
    if not isinstance(notes, dict):
        sys.exit(f"--note-file {path} must hold a JSON object {{id: note}}")
    return notes


def needs_call(raw_path: Path, note_path: str | Path | None, note: str | None, force: bool) -> bool:
    """Whether an item or frame costs an API call.

    A note decides first: a raw newer than the note file already answers it, so
    `--force --note-file` re-run with the same file buys nothing. --force only
    covers entries without a note (or runs without a note file).
    """
    raw_path = Path(raw_path)
    if note is not None and note_path is not None:
        return not raw_path.exists() or raw_path.stat().st_mtime < Path(note_path).stat().st_mtime
    return force or not raw_path.exists()


def cmd_state(args: argparse.Namespace) -> None:
    prefix = pet_prefix()
    ids = dict(zip(STATES, state_frame_ids()))
    wanted = [s.strip() for s in args.states.split(",") if s.strip()] if args.states else list(STATES)
    unknown = [s for s in wanted if s not in ids]
    if unknown:
        sys.exit(f"unknown states: {unknown}; known: {', '.join(STATES)}")

    notes: dict | None = load_notes(args.note_file) if args.note_file else None

    work = Path(args.work)
    (work / "raw").mkdir(parents=True, exist_ok=True)
    (work / "frames").mkdir(parents=True, exist_ok=True)
    failed: list[str] = []
    for n, state in enumerate(wanted, 1):
        fid = ids[state]
        note = None
        raw_path = work / "raw" / f"{fid}.png"
        frame_path = work / "frames" / f"{fid}.png"
        if notes is not None:
            # Feedback run: only frames with a note other than "ok" are redrawn;
            # a frame that is absent from the file is left alone.
            if _is_ok(notes.get(fid)):
                print(f"[{n}/{len(wanted)}] {fid}: skipped")
                continue
            note = notes[fid].strip()
        generate = needs_call(raw_path, args.note_file, note, args.force)
        try:
            if generate:
                print(f"[{n}/{len(wanted)}] {fid}: generating...", flush=True)
                raw_path.write_bytes(
                    call_api(state_refs(prefix, state, work), state_prompt(state, note),
                             args.quality, CANVAS)
                )
            elif not frame_path.exists():
                print(f"[{n}/{len(wanted)}] {fid}: cutting cached raw")
            else:
                # Never re-cut over an existing frame: that would undo `simplify`.
                print(f"[{n}/{len(wanted)}] {fid}: cached")
                continue
            cut_pet(Image.open(raw_path).convert("RGBA"), idle_box(prefix)).save(frame_path)
        except Exception as exc:   # one bad frame must not abandon the others
            failed.append(fid)
            print(f"[{n}/{len(wanted)}] {fid}: FAILED - {type(exc).__name__}: {str(exc)[:300]}",
                  flush=True)
    if failed:
        print(f"\n{len(failed)} frame(s) failed: {', '.join(failed)}")
        sys.exit(1)


def cmd_simplify(args: argparse.Namespace) -> None:
    """Merge enclosed specks in frames/<fid>.png, in place."""
    work = Path(args.work)
    ids = [f.strip() for f in (args.only or "").split(",") if f.strip()]
    if not ids:
        sys.exit("simplify needs --only frame,ids")
    for fid in ids:
        path = work / "frames" / f"{fid}.png"
        if not path.exists():
            sys.exit(f"{fid}: no {path}")
        merge_islands(Image.open(path).convert("RGBA"), args.min_island).save(path)
        print(f"{fid}: simplified")


# ---------------------------------------------------------------------------
# Assembly
# ---------------------------------------------------------------------------


def cmd_assemble(args: argparse.Namespace) -> None:
    idx = frame_index()
    work = Path(args.work)
    sheet = Image.open(PET_SHEET).convert("RGBA")
    pasted = 0
    # Honour --only: pasting every frame overwrites hand-edited artwork in rows
    # that were never regenerated.
    for fid in select(idx, args):
        info = idx[fid]
        path = work / "frames" / f"{fid}.png"
        if not path.exists():
            continue
        x, y, w, h = info["rect"]
        art = Image.open(path).convert("RGBA")
        assert art.size == (w, h), f"{fid}: {art.size} != {(w, h)}"
        # Overwrite the slot outright — paste() with a mask would keep stale art
        # underneath wherever the new frame is transparent.
        sheet.paste(art, (x, y))
        pasted += 1
    sheet.save(PET_SHEET)
    print(f"wrote {PET_SHEET} ({pasted} frames)")


# ---------------------------------------------------------------------------


def select(idx: dict, args: argparse.Namespace) -> list[str]:
    if not args.only:
        return list(idx)
    names = [f.strip() for f in args.only.split(",") if f.strip()]
    missing = [f for f in names if f not in idx]
    if missing:
        sys.exit(f"unknown frame ids: {missing}")
    return names


def main() -> None:
    default_work = os.environ.get("BORNPETS_WORK", str(REPO / f"target/{PET}-gen"))
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--work", default=default_work, help="working directory")
    p.add_argument("--only", help="comma-separated frame ids to process")
    p.add_argument("--key-file", help="file holding the API key (outside the repo, chmod 600)")
    sub = p.add_subparsers(dest="cmd", required=True)

    a = sub.add_parser("anchor")
    a.add_argument("--quality", default="medium", choices=["low", "medium", "high"])
    a.add_argument("--ref", action="append", default=[], help="extra reference image (repeatable)")
    a.set_defaults(func=cmd_anchor)

    g = sub.add_parser("gen")
    g.add_argument("--quality", default="medium", choices=["low", "medium", "high"])
    g.add_argument("--force", action="store_true", help="regenerate cached frames")
    g.set_defaults(func=cmd_gen)

    s = sub.add_parser("strip", help="one request -> N-panel image -> N frames")
    s.add_argument("strip", choices=sorted(STRIPS))
    s.add_argument("--quality", default="medium", choices=["low", "medium", "high"])
    s.add_argument("--force", action="store_true", help="regenerate a cached strip")
    s.add_argument("--reslice", action="store_true", help="re-cut a cached strip, no API call")
    s.set_defaults(func=cmd_strip)

    sub.add_parser("quantize").set_defaults(func=cmd_quantize)
    sub.add_parser("assemble").set_defaults(func=cmd_assemble)

    st = sub.add_parser("state", help="state frames from the pet's own shipped frames")
    st.add_argument("--states", help="comma-separated states (default: all ten)")
    st.add_argument("--quality", default="medium", choices=["low", "medium", "high", "max"])
    st.add_argument("--force", action="store_true", help="regenerate cached raws")
    st.add_argument("--note-file", help="JSON {frame_id: note}; only non-ok entries are redrawn")
    st.set_defaults(func=cmd_state)

    si = sub.add_parser("simplify", help="merge enclosed specks of frames/<id>.png in place")
    si.add_argument("--only", dest="only", default=argparse.SUPPRESS)
    si.add_argument("--min-island", type=int, default=3)
    si.set_defaults(func=cmd_simplify)

    ls = sub.add_parser("list")
    ls.add_argument("--states", action="store_true", help="print the ten state frame ids")
    ls.set_defaults(func=lambda a: print(",".join(state_frame_ids()) if a.states
                                         else "\n".join(frame_index())))

    args = p.parse_args()
    global KEY_FILE
    if args.key_file:
        KEY_FILE = args.key_file
    args.func(args)


if __name__ == "__main__":
    main()
