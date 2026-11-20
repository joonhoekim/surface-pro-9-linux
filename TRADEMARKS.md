# Name and trademark policy

The licences (see [LICENSE](LICENSE) and [NOTICE](NOTICE)) cover the code and the documentation. They do not cover the
project's name, its icon or the identifiers that point at this repository. This page says how those may be used. Its
purpose is to keep users from mistaking someone else's build for this project's; it does not restrict what the
licences allow you to do with the code.

"The project name" below means `surface-pro-9-linux` and the names of its components as published here
(`sp9-helperd`, `sp9-ctl`, `sp9-settings`, `sp9-digitizerd`). "The icons" means the website's icon and the
settings app's icon.

## What you may do

1. **Redistribute unmodified code from this repository under the project name**, for free or for a fee, as long as
   you say where it comes from and do not suggest that you are the project.
2. **Say that your work is based on this project**: "based on surface-pro-9-linux", "uses the surface-pro-9-linux
   kernel patches" and similar factual statements are fine.
3. **Link to the project and write about it**, including reviews and tutorials.

## What needs a different name

4. **Modified builds, builds with different defaults, and forks use their own name and icons.** Do not call them
   "surface-pro-9-linux", "official", or anything that reads as this project.
5. **Forks change the identifiers that point at this project**, all defined under [`helper/data/`](helper/data/)
   and referenced from the helper's code, `helper/install.sh` and the NixOS module:
   - the D-Bus names `io.github.joonhoekim.OpenDeviceHelper1` and `io.github.joonhoekim.Sp9Digitizer1`, and their
     bus policy and service files;
   - the polkit action prefix `io.github.joonhoekim.opendevicehelper.sp9` and the `<vendor>` of the policy file;
   - the application ID `io.github.joonhoekim.OpenDeviceHelper.SurfacePro9` (desktop file, icon, AppStream
     metainfo) and the homepage URL in the metainfo.

   Two installations that use the same names replace each other's services and policies, and a user cannot tell
   which one asked for a password.

## Other companies' names

6. This project is **not affiliated with, endorsed by or sponsored by Microsoft, Intel, Realtek or the linux-surface
   project.** "Microsoft", "Surface", "Surface Pro", "Slim Pen", "Type Cover" and "Windows" are trademarks of the
   Microsoft group of companies; "Intel" and "Iris" are trademarks of Intel Corporation or its subsidiaries;
   "Realtek" is a trademark of Realtek Semiconductor Corp. The project uses these names only to identify the device
   and the software it works with, and so should anyone using this project's work. Do not use their logos or product
   artwork with it; this project's icons are its own drawings.

If you are unsure whether a use is fine, open an issue and ask.
