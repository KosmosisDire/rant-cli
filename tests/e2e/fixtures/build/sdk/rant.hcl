package {
  name  = "sdk"
  build = ["cmake -E echo building-sdk", "cmake -E touch sdk.built"]
}
