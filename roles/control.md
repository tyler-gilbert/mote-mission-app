## Control

Understand and improve the robot control system.

Startup Tasks

- Review `mote/mote-api/src/work/control.rs`.
  - This file implements the control algorithm.
  - It accepts a guidance heading and distance as the input.
  - It must compute the wheel speed of each wheel.
- Review `mote/mote-firmware/src/tasks/gnc.rs`
  - This is the GNC firmware task that calls the work/control code
  - This file is informational but does not need to be modified.
- Run the simulation test.
  - `spaces run //mote-mission-app:sim_control`

Training Tasks

- Improve the control algorithm to operate as quickly and safely as possible.
  - Use the simulation test to check changes
  - Work with Ops to test changes on the robot

Operation Tasks.

- Go/No Go Control system readiness
