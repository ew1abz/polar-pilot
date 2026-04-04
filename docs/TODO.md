# TODO


create a github repo card
add release to github actions
merge to master release a first version
Replace manufacturer Custom to github url
Add screen switch - polar screen, big font - az and el only, info - ip, version, name, github page
Add a beautiful Rust tui companion application to control the rotator
Add a web page written in Rust/wasm as a companion app
Create a few screencasts to promote the app
Create a pitch to show why this project better then others
Use RPico for the same project.
Add real peacture
Create Amazon BOM
    Box Serpac 151 BK
    Base PCB - breadboard 7x9 cm
    Motor drivers
    W5500 module
    Nucleo board
    5 nav joystic
    SSD1306 module
    Power connector
    PoE adaptors
    Standoffs

Schematic
Test Github actions


Add cargo feature for single-axis (AZ-only) build


## Implement 1.5 Rotation

adding overtravel (often called 540° rotation) is a very common and highly effective upgrade for satellite rotators. Allowing 1.5 rotations is exactly how high-end commercial and amateur rotators handle the "dead zone" problem.
Does it help?

Yes, significantly. It reduces the number of "unwinds" you have to perform. If a satellite is crossing your 0° north mark, a standard 360° rotator has to spin all the way around immediately. With a 540° rotator (0° to 540°), you can track that satellite from 350° through 360/0° and all the way to 180° before you ever hit a physical limit.
How to Implement 1.5 Rotation

To make this work, you need to update three layers of your stack:

1. Mechanical & Hardware

    The Cable Loop: Ensure you have a "service loop" (extra slack) in your coax cables. It must be long enough to handle 540° of twist without tensioning the connectors or rubbing against sharp edges.

    Soft Limits: Update your firmware to allow an azimuth range from 0° to 540°.

2. The Logic (The "Overlap")

In this setup, your rotator essentially has "overlapping" degrees.

    0° to 180° is the primary range.

    180° to 360° is the secondary range.

    360° to 540° is the "overlap" range (which physically corresponds to 0° to 180° again).

3. Software (Hamlib / rotctld)

This is the most important step. gpredict doesn't inherently know your rotator can go past 360°. You must configure rotctld (the middleman) with the -m (min azimuth) and -M (max azimuth) flags.

    Example Command:
    rotctld -m 0 -M 540 -model [YOUR_MODEL_ID]
