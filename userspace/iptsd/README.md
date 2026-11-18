# userspace/iptsd

`0001-zero-force-contact.patch` applies to iptsd 3.1.0: the pen counts as touching when its tip signal reaches a
fraction of the level it has while it writes with force, even while it reports no force, and a pressure floor
is reported meanwhile (`[DFT] ZeroForceContact`, `ZeroForceEnter`, `ZeroForceExit`, `ZeroForceReference`,
`ContactMinPressure`; all off by default). The NixOS module applies it to `pkgs.iptsd`; the device helper writes
the values.
