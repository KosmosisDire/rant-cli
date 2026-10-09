using Rant;
using Rant.Types;

// Drives a point around the unit circle and publishes where it is.
using var node = new RantNode("talker");
var pose = node.Publisher<Pose2D>("pose");
for (double t = 0; ; t += 0.1)
{
    pose.Send(new Pose2D { Position = new Double2 { X = Math.Cos(t), Y = Math.Sin(t) }, Angle = t + Math.PI / 2 });
    Thread.Sleep(500);
}
