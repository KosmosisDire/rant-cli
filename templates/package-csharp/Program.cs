using Rant;

using var node = new RantNode("{{name}}");
var count = node.Publisher<double>("{{name}}/count");
for (double i = 0; ; i++)
{
    count.Send(i);
    Thread.Sleep(1000);
}
