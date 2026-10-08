group {
  description = "What every robot runs"

  param "range" {
    type        = int
    default     = 30
    description = "How far the lidar looks"
  }

  node "lidar" {
    type = "demo/sensor"
    args = "--pub scan --var range_${param.range}"
  }

  node "odom" {
    type = "demo/sensor"
    args = "--pub odom"
  }
}
