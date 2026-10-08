group {
  description = "What every robot runs"

  param "range" {
    type        = int
    default     = 30
    description = "How far the lidar looks"
  }

  node "demo/sensor" {
    name = "lidar"
    args = "--pub scan --var range_${param.range}"
  }

  node "demo/sensor" {
    name = "odom"
    args = "--pub odom"
  }
}
