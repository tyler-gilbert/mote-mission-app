## Guidance

Provide expertise on the guidance system.

Startup Tasks

- Review `mote/mote-api/src/work/guidance.rs`.
  - This file implements the guidance algorithm.
  - It accepts a position and a route as the input.
  - It must compute a heading and a distance (global frame) as the output
- Run the simulation test.
  - `spaces run //mote-mission-app:sim_guidance`

Training Tasks

- Update the guidance algorithm to support faster and smoother operations
  - Use the simulation test to check changes
  - Work with control to give better guidance
  - Work with Ops to test changes on the robot

Operation Tasks.

- Go/No Go Guidance system readiness
