# Operation Epic Delivery

7 Teams, High Risk Mission, Deliver Critical Supplies from FOB to Airfield deep in enemy territory to supply troops that have been cut off.

Fly a low altitude mission to avoid radar.

Positioning using two known beacons. One in the southwest and another in the northeast.

Team Makeup:

Commander: Executive decision maker
Navigation Officer: In charge of navigation solution for autonomous mote. Knowing where you are.
Guidance Officer: Full ownership of the route. Receives position from guidance and charts a course.
Control Officer: Controls the mote. Takes input from guidance and position to fly the route.
Ops Officer: Remotely operates the reliable mote. Flashes the firmware. Troubleshoots any hardware issues. Ensures batteries are charged. Ensures all operations go off without a hitch.
Software Specialist: Assigned to G/N/C teams as needed.

Work:

Commander:
- Lead your team to success. You have full authority to change anyone's role. Help out where needed. Ensure your people have what they need to succeed. Resolve disputes. Make decisions.

Ops Officer. 
- Ensure Tail number is clearly visible.
- Use USB Serial configuration to assign tail number to reliable mote.
- Ensure firmware can be flashed on the reliable mote.
- Understand how to operate the mission software.
- Ensure test arena is operating correctly.
- Execute test runs requested by G/N/C officers
- Execute the mission

Navigation Officer.
- Solve navigation work solution in mote/mote-api/src/work/navigation.rs - run the simulator. Tune the tests.
- Search out improvements to the navigation solution.
- Understand the IMU and LiDAR scanning and how you get your position in case Ops needs your support.

Guidance Officer.
- Solve guidance work solution in mote/mote-api/src/work/guidance.rs - run the simulator. Tune the tests.
- Search out improvements to the guidance solution.
- Understand how the routes are defined. Know your guidance algorithm.
- Provide guidance support to Ops

Control Officer.
- Solve control work solution in mote/mote-api/src/work/control.rs - run the simulator. Tune the tests.
- Search out improvements to the control solution. Can you go faster?
- Understand how the drive system works.
- Provide control support to Ops

Software Specialist
- Assist G/N/C officers as assigned by your commander
