# Measuring four simulated cameras at 30 fps

Budget for one live 1080p30 source plus thumbnails: about 10% of one core and 150 MB.

## Switch it on (dev build, no cameras needed)

1. In the app's web inspector console: `localStorage.setItem('kairo-dev','1')`, then open Settings > Director mode.
   A row "Simulate 4 cameras at 30 fps" appears. Switch it on; the window reloads.
   (Or open the page once with `?simcams=1`; `?simcams=0` switches it off.)
2. Four inputs, Sim 1 to Sim 4, appear in the Inputs grid at 1920 x 1080 @ 30. Switching it off removes them.
   Display windows opened afterwards capture their own simulated stream at the input's size, as they would a real camera.

The simulated picture is drawn on a canvas, so its own cost is part of every number (about 1 core-percent
per 1080p30 stream). Subtract a run of the same page with the simulation off to see the video cost alone.

## Run

Quit Safari, other WebKit apps and any other Kairo copy first, then:

    scripts/measure-video.sh 30 "label"

Scenarios, each after 10 s of settling:

| Label | Setup |
|---|---|
| 0 | Director mode on, simulation off (baseline) |
| 1 | simulation on, 4 thumbnails visible, nothing on air |
| 2 | + Sim 1 on Program (a display window open) |
| 3 | + Sim 2 on Preview |
| 4 | + Sim 3 and Sim 4 on other display windows (one per display), if you have the displays |

Dropped frames and the real frame rate of every video in the operator window and its frames (console):

    await KairoVideoStats.run(10)   // [{where, label, size, fps, dropped}, ...]

Display windows are separate webviews: open their inspector and run the same call there.
