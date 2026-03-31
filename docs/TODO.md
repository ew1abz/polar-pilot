# TODO

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
