// The smoke module: a bounded counter that exercises typechecking, simulation and Apalache
// end to end. It owns no F-n and models nothing of the kernel.
module smoke {
  var count: int

  action init = count' = 0

  action step = any {
    all { count < 3, count' = count + 1 },
    all { count == 3, count' = 0 },
  }

  val Inv_Bounded = 0 <= count and count <= 3
}
