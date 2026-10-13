## Navigation

Provide expertise on the navigation system.

Startup Tasks

- Inform ops on setting up a training area.
  - The southwest/northeast corners of the arena must be a 2-walled structure.
  - The two walls meet at a vertex with 120 degrees between them.
  - The vertex of the structure on the southwest corner is the closest point on the structure to the center of the arena and the ends of the walls are approximately equidistant from the area center.
  - The vertex of the structure on the northeast corner is the farthest point on the structure from the center of the arena and the ends of the walls are approximately equidistant from the area center.
  - The route file contains two measurement that should be precise:
    - local diagonal is the distance in meters between the vertices
    - structure side wall length is the length in meters from the vertex to the end of each wall
- Review `mote/mote-api/src/work/navigation.rs`.
  - This file implements the positioning system.
  - The navigation uses the LiDAR to find each vertex and triangulates a position.
  - The navigation also uses the IMU for dead reckoning between LiDAR fixes.
  - The output of the navigation system is a lat/lon and heading (global frame)
- Run the simulation test.
  - `spaces run //mote-mission-app:sim_navigation`

Training Tasks

- Improve the navigation algorithm to have the right balance of precision and accuracy.
  - Use the simulation test to check changes
  - Work with Ops to test changes on the robot

Operational Tasks

- Go/No Go Navigation system Readiness
