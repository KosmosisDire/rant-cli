using Rant;
using Rant.Types;

// Prints every pose the talker publishes.
using var node = new RantNode("listener");
var pose = node.Subscriber<Pose2D>("pose");
while (pose.TryTake(out Pose2D p, -1))
    Console.WriteLine($"x {p.Position.X:F2}  y {p.Position.Y:F2}  heading {p.Angle:F2} rad");
