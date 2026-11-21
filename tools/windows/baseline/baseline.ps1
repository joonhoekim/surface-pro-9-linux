# SPDX-License-Identifier: MIT
# Copyright (C) 2026 Joonhoe Kim
# Windows baseline test tool. Collects logs automatically while the docs/test-plan.md IDs are run on Windows.
#   baseline.ps1 -List                     IDs with a one-line description
#   baseline.ps1 -Test PEN-1,PEN-2         interactive run
#   baseline.ps1 -Area PEN                 a whole area
#   baseline.ps1 -Test DSP-2 -DryRun       unattended: waits -StepSeconds at each step and goes on (records no result; steps that make sound only with -AllowSound)
#   baseline.ps1 -Auto [-AllowSound]       measures only the scriptable parts and records the result (asks nothing). Scope is $AutoPlan; narrow it with -Test/-Area
# Raw data goes to private/windows/baseline/<ID>/<time>/, the summary to hw/windows/baseline/<ID>.md, the result line to hw/windows/baseline/results.md.
# Changes no system setting. Exception: during audio tests it sets the speaker endpoint volume and restores it afterwards.
param(
    [switch]$List,
    [string[]]$Test,
    [string]$Area,
    [switch]$DryRun,
    [int]$StepSeconds = 3,
    [switch]$AllowSound,
    [switch]$NoElevated,
    [switch]$Auto
)
$ErrorActionPreference = 'Continue'
. (Join-Path $PSScriptRoot 'common.ps1')
[Console]::OutputEncoding = New-Object Text.UTF8Encoding $false
$OutputEncoding = [Console]::OutputEncoding
$Utf8 = New-Object Text.UTF8Encoding $false
$Inv = [Globalization.CultureInfo]::InvariantCulture

# ---------------------------------------------------------------- test definitions
# Caps: pointer | events | hid | hid-elev | bt | battery | perf | sensors | brightness | presence | power-mode | winevents
# Info (snapshots at start and end): display | pointerdevs | hidcaps-ble | camlist | audiodev | btdevs | wlan | sam-events | penbattery
# Steps: a string = a manual step, ended with Enter. @{Say=...; Run={...}} = runs automatically, then the next step.
# Na: no corresponding test on Windows (with the reason). Manual: collects nothing, only asks for the result.
function S([string]$say, [scriptblock]$run) { @{ Say = $say; Run = $run } }

$Tests = @(
    # ---- PEN
    @{ Id = 'PEN-1'; Name = 'Pressure range and stroke-start pressure'; Needs = 'pen'; Caps = 'pointer'; Info = 'pointerdevs'
        Do = 'Light touch → hard press on the canvas 10 times, the lightest stroke 10 times. Raw pressure (0..4096/0..65535) and stroke-start pressure'
        Steps = @('Touch the canvas lightly with the pen as if making a dot, press all the way down hard and lift. 10 times.', '[clear] Draw a line with the lightest force 10 times (the least force at which a line starts).') }
    @{ Id = 'PEN-2'; Name = 'Tilt'; Needs = 'pen, protractor'; Caps = 'pointer'; Info = 'pointerdevs'
        Do = 'Held at 30/45/60° against a protractor in four directions. Mean and deviation of tiltX/Y per step'
        Steps = @(foreach ($a in 30, 45, 60) { foreach ($d in 'the right (+X)', 'the left (-X)', 'the bottom of the screen (+Y, toward you)', 'the top of the screen (-Y)') { "With the pen tip on the screen, tilt the pen ${a}° toward $d, hold it for 3 s, then press Enter." } }) }
    @{ Id = 'PEN-3'; Name = 'Side button and false detection'; Needs = 'pen'; Caps = 'pointer'
        Do = 'Side button 10 times, then 30 s of drawing without the button. Press count and false detections while drawing'
        Steps = @('With the pen tip near the screen (hovering), press and release the side button 10 times.', '[clear] Draw continuously for 30 s without touching the button.') }
    @{ Id = 'PEN-4'; Name = 'Eraser'; Needs = 'pen'; Caps = 'pointer'
        Do = 'Touch with the eraser end. Eraser/Invert flags'
        Steps = @('Turn the pen over and rub the canvas with the eraser end 5 times (a red line means it is recognised as an eraser).') }
    @{ Id = 'PEN-5'; Name = 'BLE connection and battery'; Needs = 'pen (starting asleep)'; Caps = 'bt', 'presence', 'events'; Info = 'penbattery', 'btdevs'; Elevated = $true
        Do = 'Time to reconnect after waking a sleeping pen with the tail button, pen battery %, BLE ETW'
        Steps = @('Leave the pen unused for a few minutes so it falls asleep (it is asleep when its device node disappears). Enter when ready.', 'Press the tail button once to wake the pen. Press the pen on the screen and press Enter as soon as it responds (length of this step = reconnection time).', 'Check the pen battery % in Settings > Bluetooth & devices, then Enter.') }
    @{ Id = 'PEN-6'; Name = 'Tail button'; Needs = 'pen (awake)'; Caps = 'events', 'hid', 'presence'; Kbd = $true
        Do = 'Tail button single, double and long press. Key combinations arriving on the BLE keyboard collection'
        Steps = @('Press the tail button once (close the app that opens).', 'Press the tail button twice.', 'Hold the tail button down.') }
    @{ Id = 'PEN-7'; Name = 'Pen reports through the digitizer (Col07)'; Needs = 'pen'; Caps = 'pointer', 'hid-elev'; Elevated = $true
        Do = 'Whether THC Col07 (FFF4) input reports 84/110 arrive (elevated hidapi, address bytes in private/ only)'
        Steps = @('Draw on the screen with the pen for 10 s.', 'Move the pen well away from the screen and wait 5 s.') }
    @{ Id = 'PEN-8'; Name = 'Firmware collection input (the Windows side of duplicate input)'; Needs = 'pen'; Caps = 'pointer'
        Do = 'Whether Raw Input reports arrive through THC Col09 (pen) and Col06 (touch) or through the HEAT virtual devices'
        Steps = @('Draw with the pen for 10 s.', 'Draw with a finger for 10 s.') }
    @{ Id = 'PEN-9'; Name = 'Haptics'; Needs = 'pen (awake)'; Caps = 'bt', 'hid', 'pointer'; Info = 'hidcaps-ble'; Elevated = $true
        Do = 'BLE ETW and pen BLE HID capture during haptics (to see the report 65 that Windows sends)'
        Steps = @('In Settings > Bluetooth & devices > Pen & Windows Ink, only check that "Tactile signals" is on (do not change it). Enter.', 'Draw and erase for 20 s in an app with haptics (Whiteboard, OneNote and others). Remember whether you felt vibration.', 'Put the pen down, then Enter.') }
    # ---- TOUCH
    @{ Id = 'TOUCH-1'; Name = 'Missed light taps and swipes'; Needs = ''; Caps = 'pointer'; Info = 'pointerdevs'
        Do = 'Light fingertip taps 20 times, three-finger swipes 10 times. Recognised counts'
        Steps = @('Tap the canvas lightly with a fingertip 20 times.', '[clear] Swipe with three fingers inside the canvas 10 times (if a system gesture takes it, note that).') }
    @{ Id = 'TOUCH-2'; Name = 'Palm rejection'; Needs = 'pen'; Caps = 'pointer'
        Do = 'Palm contact while the pen hovers. Number of touches while the pen is in proximity'
        Steps = @('With the pen hovering over the screen, put your palm on the screen 5 times.', '[clear] Put your palm on the screen 5 times without the pen (for comparison).') }
    @{ Id = 'TOUCH-3'; Name = 'Recovery after standby'; Needs = 'pen'; Caps = 'pointer', 'winevents', 'events'
        Do = 'Modern Standby entry and exit 10 times, touch and pen each time. Kernel-Power 506/507'
        Steps = @('Enter standby with the power button and resume with the power button, 10 times. Each time draw a line on the canvas with a finger and with the pen. Enter when done.') ; Timeout = 1800 }
    @{ Id = 'TOUCH-4'; Name = 'Wake by touch'; Needs = 'pen'; Caps = 'events', 'winevents'
        Do = 'Whether touch or the pen wakes the device from screen-off or standby'
        Steps = @('Enter standby with the power button, then tap the screen and touch it with the pen. If it does not wake, wake it with the power button. Enter.') }
    # ---- CAM
    @{ Id = 'CAM-1'; Name = 'Rear capture and orientation'; Needs = ''; Caps = @(); Info = 'camlist'
        Do = 'Rear 1920x1080 and 3840x2160, 3 s capture each, measured fps, last frame saved and its orientation checked'
        Steps = @((S 'Capturing from the rear camera' { Invoke-Cam Rear 1920x1080 30 3; Invoke-Cam Rear 3840x2160 30 3 -Show }), 'Check that the opened picture is upright. Enter.') }
    @{ Id = 'CAM-2'; Name = 'Front capture'; Needs = ''; Caps = @(); Info = 'camlist'
        Do = 'Front 1920x1080 and 1920x1440 capture, measured fps, IR camera modes (WinRT)'
        Steps = @((S 'Capturing from the front camera' { Invoke-Cam Front 1920x1080 30 3; Invoke-Cam Front 1920x1440 30 3 -Show }), 'Check that the opened picture is upright. Enter.') }
    @{ Id = 'CAM-3'; Name = 'Streaming per mode'; Needs = ''; Caps = @(); Info = 'camlist'
        Do = 'Capture 2 s at every resolution the front and rear cameras offer (Windows runs the front sensor in the single 1296x972 binned mode and the ISP scales it up)'
        Steps = @((S 'Capturing every mode in turn (1–2 min)' { Invoke-CamAll })) }
    @{ Id = 'CAM-4'; Name = 'Autofocus'; Needs = 'a near object (10 cm), a far background'; Caps = @()
        Do = 'Rear captures near and far, sharpness (variance of the Laplacian) compared'
        Steps = @('Put an object with text on it 10 cm in front of the rear camera. Enter.', (S 'Near capture' { Invoke-Cam Rear 1920x1080 30 4 -Tag near -Sharp }), 'Remove the object and point the camera at a background at least 2 m away. Enter.', (S 'Far capture' { Invoke-Cam Rear 1920x1080 30 4 -Tag far -Sharp })) }
    @{ Id = 'CAM-5'; Name = 'Privacy LED'; Needs = ''; Caps = @()
        Do = 'Observe which LED lights while streaming 10 s from the front and from the rear'
        Steps = @((S 'Front streaming for 10 s: watch the LEDs on the device' { Invoke-Cam Front 1280x720 30 10 }), (S 'Rear streaming for 10 s: watch the LEDs' { Invoke-Cam Rear 1280x720 30 10 }), 'Enter when ready to note what you saw.') }
    @{ Id = 'CAM-6'; Name = 'Image quality'; Needs = 'the same indoor lighting'; Caps = @()
        Do = 'Save front and rear frames at the highest resolution to private/ (to compare side by side with the Linux pictures)'
        Steps = @((S 'Saving frames at the highest resolution' { Invoke-Cam Front 1920x1440 30 3 -Tag quality; Invoke-Cam Rear 3840x2160 30 3 -Tag quality }), 'Optionally put Windows Camera app pictures into the same folder. Enter.') }
    # ---- SAM
    @{ Id = 'SAM-1'; Name = 'Type Cover detach/attach'; Needs = 'Type Cover'; Caps = 'events', 'presence', 'winevents'
        Do = 'Detach and attach the cover 5 times. Device node and tablet mode changes'
        Steps = @('Detach and attach the cover 5 times. Check typing, the touchpad and the backlight key each time. Enter.') }
    @{ Id = 'SAM-2'; Name = 'Tablet mode'; Needs = 'Type Cover'; Caps = 'events', 'presence'
        Do = 'Fold the cover back, detach it. ConvertibleSlateMode changes'
        Steps = @('Fold the cover back. Enter.', 'Unfold the cover into the typing position. Enter.', 'Detach the cover. Enter.', 'Attach the cover. Enter.') }
    @{ Id = 'SAM-3'; Name = 'Buttons'; Needs = ''; Caps = 'events', 'winevents'
        Do = 'Volume up/down 5 times each, standby and resume with the power button. Raw Input consumer/system reports'
        Steps = @('Volume up button 5 times.', 'Volume down button 5 times.', 'Enter standby with the power button and wake with the power button. Enter.') }
    @{ Id = 'SAM-4'; Name = 'Fan after load (preparing the Linux reboot)'; Needs = 'charger'; Caps = 'perf', 'winevents'
        Do = '5 min of all-core load on Windows. Then reboot straight into Linux and check the fan RPM'
        Steps = @((S '5 min all-core load' { Invoke-Stress 300 }), 'Now reboot into Linux and read sensors after 5 min idle (Linux SAM-4). Enter.') }
    @{ Id = 'SAM-5'; Name = 'Performance per power mode'; Needs = 'charger'; Caps = 'perf', 'power-mode', 'winevents'
        Do = '120 s of load in each of the 3 Windows power modes. Effective clock, package W, temperature'
        Steps = @('In Settings > System > Power, set the power mode to "Best Power Efficiency". Enter.', (S '120 s load' { Invoke-Stress 120 }), 'Power mode "Balanced". Enter.', (S '120 s load' { Invoke-Stress 120 }), 'Power mode "Best Performance". Enter.', (S '120 s load' { Invoke-Stress 120 }), 'Set the power mode back to what it was. Enter.') }
    @{ Id = 'SAM-6'; Name = 'Battery and charging'; Needs = 'charger'; Caps = 'battery', 'events', 'winevents'
        Do = 'Unplug and plug AC 5 times. Status updates and charge rate'
        Steps = @('Unplug and plug the charger 5 times (at least 10 s each). Enter.') }
    @{ Id = 'SAM-7'; Name = 'Rotation and posture sensors'; Needs = ''; Caps = 'sensors', 'events'
        Do = 'Four orientations + lying flat. Acceleration vector (reference for the Linux mount matrix)'
        Steps = @('Stand it in landscape, camera at the top. Enter after 3 s.', 'Rotate it 90° clockwise. Enter after 3 s.', '180° (camera at the bottom). Enter after 3 s.', '90° counter-clockwise. Enter after 3 s.', 'Lay it flat on a desk, screen up. Enter after 3 s.') }
    @{ Id = 'SAM-8'; Name = 'Observing SAM events'; Needs = 'charger, Type Cover'; Caps = 'winevents', 'events', 'perf'; Info = 'sam-events'
        Do = 'Surface event logs and SMF power limit updates during cover, charger and load changes'
        Steps = @('Detach and attach the cover. Enter.', 'Unplug and plug the charger. Enter.', (S '30 s load' { Invoke-Stress 30 })) }
    # ---- AUD
    @{ Id = 'AUD-1'; Name = 'Speakers: left/right, distortion'; Needs = 'a quiet room'; Caps = @(); Sound = $true
        Do = '1 kHz left, right and both at volume 25/50/75% (100% after confirmation), sweep at 50%. Left/right balance and THD from the microphones'
        Steps = @((S '1 kHz tone per volume step' { foreach ($v in 25, 50, 75) { Invoke-Audio tones $v '--freqs 1000' "vol$v" } ; if (Confirm-Ask 'Also play briefly at 100% volume?') { Invoke-Audio tones 100 '--freqs 1000 --level -24' 'vol100' } }),
            (S 'Sweep at 50% (left, right)' { Invoke-Audio sweep 50 }), 'Enter when ready to note whether left and right sounded right and whether there was distortion.') }
    @{ Id = 'AUD-2'; Name = 'Speaker processing (APO on/bypassed)'; Needs = 'a quiet room'; Caps = @(); Sound = $true; Setting = 'APO on and bypassed'
        Do = '40/100/3700 Hz tones and a sweep with the APO on (shared) and bypassed (exclusive playback). Compared with the Linux filter chain off/on'
        Steps = @((S 'APO on (shared mode): tones, sweep' { Invoke-Audio tones 75 '--freqs 40,100,3700' 'vol75'; Invoke-Audio sweep 50 }),
            (S 'APO bypassed (exclusive mode): tones, sweep' { Invoke-Audio tones 75 '--freqs 40,100,3700 --mode exclusive' 'vol75'; Invoke-Audio sweep 50 '--mode exclusive' }), 'Enter when ready to note the difference in sound between the two.') }
    @{ Id = 'AUD-3'; Name = 'Built-in microphones'; Needs = 'a quiet room'; Caps = @(); Sound = $true
        Do = 'Delay and polarity between the microphones from a speaker sweep; channel levels, sum − difference and clipping from 5 s of speech in front'
        Steps = @((S 'Microphone phase from a sweep' { Invoke-Audio sweep 50 }), 'From 30 cm in front of the screen, speak at normal volume for 5 s right after Enter.', (S 'Recording 5 s' { Invoke-Audio record 0 '--seconds 5' })) }
    @{ Id = 'AUD-4'; Name = 'Call echo'; Needs = 'a call partner'; Manual = $true
        Do = 'A call in a video call app with the speakers playing. Whether the other side hears echo (observation only)'; Steps = @('Test with a call app. Enter when done.') }
    @{ Id = 'AUD-5'; Name = 'Bluetooth audio'; Needs = 'Bluetooth earphones'; Caps = 'events'; Info = 'audiodev', 'btdevs'
        Do = 'A2DP playback, HFP call microphone. Endpoint list before and after'
        Steps = @('Connect Bluetooth earphones and play music. Enter.', 'Speak into the earphone microphone in a call app. Enter.') }
    @{ Id = 'AUD-6'; Name = 'External monitor audio'; Needs = 'USB-C DP/HDMI monitor (with speakers)'; Caps = 'events'; Info = 'audiodev', 'display'
        Do = 'Playback on the external monitor speakers. Endpoint list before and after'
        Steps = @('Connect the monitor and play sound on its speakers. Enter.') }
    # ---- DSP
    @{ Id = 'DSP-1'; Name = 'Screen flicker'; Needs = 'charger'; Caps = 'events', 'brightness', 'battery', 'perf'; Timeout = 1800
        Do = 'A few minutes each of a still screen, typing and scrolling, on AC and on battery. Flicker observed (PSR is not visible on Windows), package W'
        Steps = @('On battery, a still screen for 3 min. Enter.', 'On battery, typing and scrolling for 3 min. Enter.', 'With the charger plugged in, a still screen for 2 min, typing and scrolling for 2 min. Enter.') }
    @{ Id = 'DSP-2'; Name = '60↔120 Hz'; Needs = ''; Caps = 'events'; Info = 'display'
        Do = 'Switch 60↔120 Hz in Settings 5 times. Time of the mode change and whether the screen goes dark'
        Steps = @('In Settings > Display > Advanced display, switch the refresh rate 60↔120 Hz 5 times (watch whether the screen goes dark each time). End on the original value. Enter.', 'If there is a "Dynamic refresh rate" option, only note its state. Enter.') }
    @{ Id = 'DSP-3'; Name = 'Backlight minimum'; Needs = ''; Caps = 'brightness', 'events'
        Do = 'Brightness down to 0 and back up. Whether the screen turns off at 0'
        Steps = @('Turn the brightness all the way down (0) and watch whether the screen goes fully dark. Enter.', 'Raise the brightness back to where it was. Watch for flicker while adjusting. Enter.') }
    @{ Id = 'DSP-4'; Name = 'Ambient light sensor'; Needs = 'normal indoor lighting'; Caps = 'sensors'
        Do = 'Normal lighting and the sensor covered. lux'
        Steps = @('Stand the device up in normal lighting. Enter after 5 s.', 'Cover the sensor next to the front camera with a finger. Enter after 5 s.') }
    @{ Id = 'DSP-5'; Name = 'External monitor and dock'; Needs = 'USB-C monitor, charger'; Caps = 'events', 'presence', 'winevents'; Info = 'display'
        Do = 'Connect and disconnect a monitor and a charger on each of the two USB-C ports'
        Steps = @('Connect the monitor to port 1 and disconnect it once it shows a picture. Enter.', 'Connect the monitor to port 2 and disconnect it once it shows a picture. Enter.', 'Plug the charger into each port and unplug it. Enter.') }
    # ---- PWR
    @{ Id = 'PWR-1'; Name = 'Standby round trip'; Needs = ''; Caps = 'events', 'winevents', 'battery'; Timeout = 1800
        Do = 'Modern Standby entry and exit with the power button, 10 times. Kernel-Power 506/507'
        Steps = @('Enter standby with the power button and resume after 10 s, 10 times. Enter.') }
    @{ Id = 'PWR-2'; Name = 'Lid (cover) and standby'; Needs = 'charger, Type Cover'; Caps = 'events', 'winevents'
        Do = 'Close → open the cover while charging, 5 times. Lid notifications and standby entry and exit'
        Steps = @('With the charger plugged in, close the cover and open it after 10 s. 5 times. Enter.') }
    @{ Id = 'PWR-3'; Name = 'Consecutive standby after audio playback'; Needs = ''; Caps = 'events', 'winevents'; Sound = $true
        Do = 'A short sound, then standby entry and exit 3 times'
        Steps = @((S 'Playing a 1 s tone' { Invoke-Audio tones 30 '--freqs 1000 --seconds 1' 'pre-sleep' }), 'Enter standby with the power button and wake after 20 s, 3 times. Enter.') }
    @{ Id = 'PWR-4'; Name = 'Standby power'; Needs = '1 hour'; Caps = 'battery', 'winevents'; Timeout = 14400; Elevated = $true
        Do = '1 hour of standby on battery. mW from the change in charge, then sleepstudy (HW DRIPS)'
        Steps = @('Unplug the charger. Right after Enter, enter standby with the power button.', 'Wake it after about 1 hour, then Enter.', (S 'Collecting sleepstudy (elevated)' { Invoke-Elevated sleepstudy (Join-Path $script:Raw 'power') })) }
    @{ Id = 'PWR-5'; Name = 'Runtime screen-off PC10'; Na = 'Windows has no separate runtime state with only the screen off to test. HW DRIPS in standby (PWR-4 sleepstudy) is the corresponding reference value' }
    @{ Id = 'PWR-6'; Name = 'RTC wake'; Na = 'not done: on Windows it needs a wake timer to be set (a settings change)' }
    @{ Id = 'PWR-7'; Name = 'Shutdown'; Needs = 'USB hub (with LED)'; Manual = $true
        Do = 'Shut Windows down 5 times, observe the hub LED and the power LED (observation only; the tool cannot run across a shutdown)'; Steps = @('Shut down and power on 5 times while observing, then reopen this tool and record only the result. Enter.') }
    @{ Id = 'PWR-8'; Name = 'Hibernation'; Na = 'hibernation is off on this Windows install, and turning it on is a settings change' }
    @{ Id = 'PWR-9'; Name = 'Thermals and sustained performance'; Needs = 'charger'; Caps = 'perf', 'battery', 'winevents'
        Do = '10 min of all-core load. Effective clock, package W and temperature per 30 s interval, SMF PL updates'
        Steps = @((S '10 min all-core load (feel the temperature of the back by hand)' { Invoke-Stress 600 }), 'Enter when ready to note how warm the back felt.') }
    # ---- NET
    @{ Id = 'NET-1'; Name = 'Wi-Fi'; Needs = '5 GHz AP, iperf3 server (optional)'; Caps = 'events', 'winevents'; Info = 'wlan'
        Do = 'Connection state (band, channel, rate, signal), iperf3 throughput (with a server), reconnection after resume from standby'
        Steps = @((S 'iperf3' { Invoke-Iperf }), 'Enter and leave standby with the power button and check that it reconnects by itself. Enter.') }
    @{ Id = 'NET-2'; Name = 'Bluetooth'; Needs = 'mouse, earphones'; Caps = 'events', 'presence', 'winevents'; Info = 'btdevs'
        Do = 'Mouse and earphones connected, reconnection after resume from standby'
        Steps = @('Connect and use the mouse and the earphones. Enter.', 'Enter and leave standby and check that they reconnect. Enter.') }
)

# ---------------------------------------------------------------- automatic mode (-Auto)
# Only the parts measurable unattended and without external equipment. A test with no definition or with Skip is passed over as skipped.
# Every Step is S (automatic). Without Caps/Info the test definition's are used. Require: ac | battery | bt-audio | ext-monitor (Get-AutoEnv).
# Human: the manual part, left in the result line as still needing checking. Settings (brightness, refresh rate, power mode) are read, never changed.
# Never suspends, locks or powers off the device and never drops a connection. Sound only at volume 20%, -24 dBFS (-AllowSound).
$AutoAudio = '--level -24'
$AutoPlan = @{
    'CAM-1' = @{ Steps = @((S 'Rear 1920x1080 and 3840x2160, 3 s capture each' { Invoke-Cam Rear 1920x1080 30 3; Invoke-Cam Rear 3840x2160 30 3 }))
        Human = 'whether the saved rear frames (private/…/cam/*.jpg) are upright' }
    'CAM-2' = @{ Steps = @((S 'Front 1920x1080 and 1920x1440, 3 s capture each' { Invoke-Cam Front 1920x1080 30 3; Invoke-Cam Front 1920x1440 30 3 }))
        Human = 'whether the saved front frames are upright' }
    'CAM-3' = @{ Steps = @((S 'Every front and rear mode, 2 s capture each' { Invoke-CamAll })) }
    'CAM-4' = @{ Skip = 'manual: placing a near object and a far background' }
    'CAM-5' = @{ Skip = 'manual: watching the LED' }
    'CAM-6' = @{ Steps = @((S 'Saving front and rear frames at the highest resolution' { Invoke-Cam Front 1920x1440 30 3 -Tag quality; Invoke-Cam Rear 3840x2160 30 3 -Tag quality }))
        Human = 'side-by-side comparison with the Linux pictures under the same lighting' }
    'SAM-1' = @{ Skip = 'manual: detaching and attaching the Type Cover' }
    'SAM-2' = @{ Skip = 'manual: folding and detaching the cover' }
    'SAM-3' = @{ Skip = 'manual: pressing the buttons' }
    'SAM-4' = @{ Skip = 'only meaningful with a reboot into Linux right after the load (the unattended PWR-9 run covers the Windows load values)' }
    'SAM-5' = @{ Require = 'ac'; Caps = @('perf', 'power-mode', 'winevents')
        Steps = @((S 'Idle 30 s' { Wait-Auto 30 }), (S '120 s all-core load in the current power mode' { Invoke-Stress 120 }), (S 'Cool-down 30 s' { Wait-Auto 30 }))
        Human = 'the same 120 s load in the power modes "Best Power Efficiency" and "Best Performance" (the tool does not change the power mode)' }
    'SAM-6' = @{ Caps = @('battery', 'events'); Steps = @((S 'Battery state 60 s' { Wait-Auto 60 }))
        Human = 'unplugging and plugging the charger 5 times and the status updates, the charge limit state near 80%' }
    'SAM-7' = @{ Caps = @('sensors'); Steps = @((S 'Current posture 10 s' { Wait-Auto 10 }))
        Human = 'the four orientations and lying flat (acceleration vectors of the five postures)' }
    'SAM-8' = @{ Caps = @('winevents', 'events', 'perf')
        Steps = @((S 'Idle 20 s' { Wait-Auto 20 }), (S '30 s load' { Invoke-Stress 30 }), (S 'Cool-down 20 s' { Wait-Auto 20 }))
        Human = 'the events while detaching the cover and unplugging the charger' }
    'AUD-1' = @{ Steps = @((S '1 kHz tone at 20% volume (-24 dBFS)' { Invoke-Audio tones 20 "--freqs 1000 $AutoAudio" 'vol20' }), (S 'Sweep at 20% volume (left, right)' { Invoke-Audio sweep 20 $AutoAudio }))
        Human = 'listening for left/right and distortion, the 25/50/75/100% volume steps (loud sound is never played unattended)' }
    'AUD-2' = @{ Setting = 'APO on and bypassed'
        Steps = @((S 'APO on (shared): 40/100/3700 Hz tones, sweep' { Invoke-Audio tones 20 "--freqs 40,100,3700 $AutoAudio" 'vol20'; Invoke-Audio sweep 20 $AutoAudio }),
            (S 'APO bypassed (exclusive): tones, sweep' { Invoke-Audio tones 20 "--freqs 40,100,3700 $AutoAudio --mode exclusive" 'vol20'; Invoke-Audio sweep 20 "$AutoAudio --mode exclusive" }))
        Human = 'listening for the difference between the two paths at 75% or more (at low volume the difference is small)' }
    'AUD-3' = @{ Steps = @((S 'Microphone phase from a sweep at 20% volume' { Invoke-Audio sweep 20 $AutoAudio }), (S 'Recording 5 s of silence (noise floor)' { Invoke-Audio record 0 '--seconds 5' 'ambient' }))
        Human = 'a 5 s recording of speech from 30 cm in front (sum − difference, clipping)' }
    'AUD-5' = @{ Require = 'bt-audio'; Steps = @((S 'Endpoint list' { Wait-Auto 3 })); Human = 'playback on the earphones and the call microphone sound' }
    'AUD-6' = @{ Require = 'ext-monitor'; Steps = @((S 'Endpoint list' { Wait-Auto 3 })); Human = 'sound from the monitor speakers' }
    'DSP-1' = @{ Caps = @('perf', 'brightness', 'battery', 'events'); Steps = @((S 'Still screen 60 s (current power source)' { Wait-Auto 60 }))
        Human = 'watching for flicker on a still screen, while typing and scrolling; the same measurement on battery' }
    'DSP-2' = @{ Steps = @((S 'Recording the current mode 5 s' { Wait-Auto 5 }))
        Human = 'switching 60↔120 Hz in Settings 5 times (watching the screen go dark), the dynamic refresh rate state (the tool does not change the refresh rate)' }
    'DSP-3' = @{ Caps = @('brightness'); Info = @('brightness'); Steps = @((S 'Brightness 5 s' { Wait-Auto 5 }))
        Human = 'whether the screen turns off at brightness 0, flicker while adjusting (the tool does not change the brightness)' }
    'DSP-4' = @{ Caps = @('sensors'); Steps = @((S 'Normal lighting 10 s' { Wait-Auto 10 })); Human = 'whether lux drops with the sensor covered by a finger' }
    'DSP-5' = @{ Require = 'ext-monitor'; Steps = @((S 'Display list' { Wait-Auto 3 })); Human = 'connecting and disconnecting a monitor and a charger on each of the two USB-C ports' }
    'PWR-1' = @{ Skip = 'manual: needs standby entry and exit (the tool never puts the device to sleep)' }
    'PWR-2' = @{ Skip = 'manual: closing and opening the cover' }
    'PWR-3' = @{ Skip = 'manual: needs standby entry and exit' }
    'PWR-4' = @{ Caps = @('battery')
        Steps = @((S 'sleepstudy of the last day (elevated)' { Invoke-Elevated sleepstudy (Join-Path $script:Raw 'power') }),
            (S 'Idle discharge 5 min, on battery only' { if ((Get-AutoEnv).Ac) { Say '  skipped: on AC, so no discharge rate' 'DarkYellow' } else { Wait-Auto 300 } }))
        Human = 'the change in charge (mW) over 1 hour of standby with the charger unplugged' }
    'PWR-9' = @{ Require = 'ac'; Caps = @('perf', 'battery', 'winevents')
        Steps = @((S 'Idle 30 s' { Wait-Auto 30 }), (S 'All-core load 300 s' { Invoke-Stress 300 }), (S 'Cool-down 60 s' { Wait-Auto 60 }))
        Human = 'a 10 min load to check for throttle oscillation, how warm the back feels' }
    'NET-1' = @{ Caps = @(); Steps = @((S 'Reading the Wi-Fi state' { Wait-Auto 2 }))
        Human = 'iperf3 throughput (needs a server), reconnection after standby, band, channel and signal (visible only with the location service on)' }
    'NET-2' = @{ Skip = 'manual: needs a mouse and earphones connected and a resume from standby' }
}

# ---------------------------------------------------------------- common functions
function Say([string]$s, [string]$color = 'Cyan') { Write-Host $s -ForegroundColor $color }
function Confirm-Ask([string]$q) {
    if ($DryRun) { return $false }
    $a = Read-Host "$q [y/N]"; return $a -match '^(y|yes|\u3147|\uC608)$'   # \u3147, \uC608: "yes" typed with a Korean input method
}

function Wait-Next([int]$timeout) {
    if ($DryRun) { Start-Sleep -Seconds $StepSeconds; return 'dry' }
    if ([Console]::IsInputRedirected) { [void](Read-Host); return 'enter' }   # scripted input: one line per step
    $next = $script:NextFile
    if ($next) { Remove-Item $next -ErrorAction SilentlyContinue }
    $t0 = Get-Date
    while ($true) {
        if ([Console]::KeyAvailable) { $k = [Console]::ReadKey($true); if ($k.Key -eq 'Enter') { return 'enter' } }
        if ($next -and (Test-Path $next)) { Remove-Item $next -ErrorAction SilentlyContinue; return 'enter' }
        if ($timeout -gt 0 -and ((Get-Date) - $t0).TotalSeconds -gt $timeout) { return 'timeout' }
        Start-Sleep -Milliseconds 100
    }
}

function Wait-Auto([int]$sec) {
    if ($DryRun) { $sec = [math]::Min($sec, $StepSeconds) }
    Start-Sleep -Seconds $sec
}

# What is connected right now (read once per run): AC, active displays, Bluetooth audio sink, type cover.
function Get-AutoEnv {
    if ($script:AutoEnv) { return $script:AutoEnv }
    $b = Get-CimInstance -Namespace root\wmi -ClassName BatteryStatus -ErrorAction SilentlyContinue | Select-Object -First 1
    $disp = if (Test-Path $Kit.Probe) { @(& $Kit.Probe display | Where-Object { $_ -match '^display ' }).Count } else { 1 }
    $pnp = @(Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.Status -eq 'OK' })
    $script:AutoEnv = @{
        Ac       = [bool]$b.PowerOnline
        Displays = $disp
        BtAudio  = @($pnp | Where-Object { $_.InstanceId -match '^BTHENUM\\\{0000110B' }).Count -gt 0   # A2DP sink service
        Cover    = @($pnp | Where-Object { $_.InstanceId -match 'TARGET_KIP' }).Count -gt 0
    }
    return $script:AutoEnv
}

function Get-AutoSkip($t) {
    $p = $AutoPlan[$t.Id]
    if ($t.Na) { return "no Windows counterpart ($($t.Na))" }
    if ($t.Manual) { return "manual: observation only" }
    if (-not $p) { return "manual: pen or hand input" + $(if ($t.Needs) { " (needs: $($t.Needs))" }) }
    if ($p.Skip) { return $p.Skip }
    if ($t.Sound -and -not $AllowSound) { return 'makes sound, which needs -AllowSound' }
    $e = Get-AutoEnv
    switch ($p.Require) {
        'ac' { if (-not $e.Ac) { return 'charger not connected (load is measured on AC)' } }
        'battery' { if ($e.Ac) { return 'on AC' } }
        'bt-audio' { if (-not $e.BtAudio) { return 'no Bluetooth audio device connected' } }
        'ext-monitor' { if ($e.Displays -lt 2) { return 'no external monitor connected' } }
    }
    return $null
}

function Add-Marker([string]$step, [string]$text) {
    $f = Join-Path $script:Raw 'markers.csv'
    if (-not (Test-Path $f)) { [IO.File]::WriteAllText($f, "t,step,text`n", $Utf8) }
    [IO.File]::AppendAllText($f, ("{0},{1},{2}`n" -f (Get-UnixTime).ToString('F3', $Inv), $step, ($text -replace '[,\r\n]', ' ')), $Utf8)
}

function Start-Bg([string]$exe, [string[]]$argv) {
    Start-Process -FilePath $exe -ArgumentList $argv -WindowStyle Hidden -PassThru
}

function Invoke-Elevated([string]$action, [string]$dir, [string]$extra) {
    if ($NoElevated) { Say "  (elevation skipped: $action)" 'DarkYellow'; return }
    $a = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $Kit.Elev, (Join-Path $Kit.Dir 'elevated.ps1'), '-Action', $action)
    if ($dir) { $a += @('-Dir', $dir) }
    & powershell @a 2>&1 | ForEach-Object { "  $_" } | Write-Host
}

# ---- capture start/stop
function Start-Captures($t) {
    $caps = @($t.Caps | Where-Object { $_ })
    $script:Procs = @(); $script:StopDirs = @(); $script:NextFile = $null
    $script:BtRunning = $false
    if ($caps -contains 'pointer' -or $caps -contains 'events') {
        $d = Join-Path $script:Raw 'probe'; New-Item -ItemType Directory -Force $d | Out-Null
        $mode = if ($caps -contains 'pointer') { 'pointer' } else { 'events' }
        if (Test-Path $Kit.Probe) {
            $pa = @($mode, "`"$d`""); if ($t.Kbd) { $pa += '--kbd' }   # keyboards only where the tail button is tested
            $script:Procs += Start-Process -FilePath $Kit.Probe -ArgumentList $pa -PassThru
            $script:StopDirs += $d
            if ($mode -eq 'pointer') { $script:NextFile = Join-Path $d 'next' }
        } else { Say '  sp9probe.exe missing: run setup.ps1 first' 'Red' }
    }
    foreach ($k in 'battery', 'perf', 'sensors', 'brightness', 'presence', 'power-mode') {
        if ($caps -notcontains $k) { continue }
        $d = Join-Path $script:Raw 'sampler'; New-Item -ItemType Directory -Force $d | Out-Null
        $script:Procs += Start-Bg 'powershell' @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$(Join-Path $Kit.Dir 'sampler.ps1')`"", '-Kind', $k, '-Dir', "`"$d`"")
        $script:StopDirs += $d
    }
    if ($caps -contains 'hid') {
        $d = Join-Path $script:Raw 'hid'
        if (Test-Path $Kit.Python) {
            $script:Procs += Start-Bg $Kit.Python @('-I', "`"$(Join-Path $Kit.Dir 'hidlog.py')`"", '--out', "`"$d`"")
            $script:StopDirs += $d
        } else { Say '  Python venv missing: setup.ps1' 'Red' }
    }
    if ($caps -contains 'hid-elev' -and -not $NoElevated) {
        $d = Join-Path $script:Raw 'hid-elev'; New-Item -ItemType Directory -Force $d | Out-Null
        # run-elevated waits for the elevated script, so the wrapper itself runs in the background; it stops via the stop file
        $script:Procs += Start-Bg 'powershell' @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$($Kit.Elev)`"", "`"$(Join-Path $Kit.Dir 'elevated.ps1')`"", '-Action', 'hidlog', '-Dir', "`"$d`"")
        $script:StopDirs += $d
    }
    if ($caps -contains 'bt' -and -not $NoElevated) {
        if (Test-Path $Kit.Wprp) { Invoke-Elevated 'bt-start'; $script:BtRunning = $true }
        else { Say '  BluetoothStack.wprp missing: setup.ps1' 'Red' }
    }
    # Wait until every backend has written its first data row (WinRT/Get-Counter take a few seconds to start),
    # so the first step is covered.
    $want = @()
    if ($caps -contains 'pointer' -or $caps -contains 'events') { $want += Join-Path $script:Raw 'probe\events.csv' }
    foreach ($k in 'battery', 'perf', 'sensors', 'brightness', 'presence', 'power-mode') { if ($caps -contains $k) { $want += Join-Path $script:Raw "sampler\$k.csv" } }
    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        $ready = @($want | Where-Object { (Test-Path $_) -and @(Get-Content $_ -TotalCount 2 -ErrorAction SilentlyContinue).Count -ge 2 })
        if ($ready.Count -ge $want.Count) { break }
        Start-Sleep -Milliseconds 250
    }
    Start-Sleep -Milliseconds 500
}

function Stop-Captures($t) {
    foreach ($d in $script:StopDirs | Select-Object -Unique) { New-Item -ItemType File -Force (Join-Path $d 'stop') | Out-Null }
    $deadline = (Get-Date).AddSeconds(10)
    foreach ($p in $script:Procs) {
        $left = [int][math]::Max(1, ($deadline - (Get-Date)).TotalSeconds)
        try { $p | Wait-Process -Timeout $left -ErrorAction Stop } catch { try { $p | Stop-Process -Force -ErrorAction SilentlyContinue } catch {} }
    }
    if ($script:BtRunning) { Invoke-Elevated 'bt-stop' (Join-Path $script:Raw 'bt'); $script:BtRunning = $false }
    if (@($t.Caps) -contains 'winevents') { Export-WinEvents (Join-Path $script:Raw 'win') $script:T0 }
}

# ---- event log
function Export-WinEvents([string]$dir, [datetime]$since) {
    New-Item -ItemType Directory -Force $dir | Out-Null
    $out = New-Object Collections.Generic.List[string]; $out.Add('t,log,id,what,value')
    $what = @{ 506 = 'Modern Standby entry'; 507 = 'Modern Standby exit'; 42 = 'sleep entry'; 107 = 'sleep exit'; 1 = 'wake from sleep (Power-Troubleshooter)'; 109 = 'shutdown start'; 41 = 'unexpected shutdown'; 105 = 'power source change' }
    $ev = Get-WinEvent -FilterHashtable @{ LogName = 'System'; StartTime = $since } -ErrorAction SilentlyContinue |
        Where-Object { ($_.ProviderName -match 'Kernel-Power|Power-Troubleshooter') -and $what.ContainsKey($_.Id) }
    foreach ($e in $ev) { $out.Add(('{0},System,{1},{2},' -f ([DateTimeOffset]$e.TimeCreated).ToUnixTimeMilliseconds() / 1000, $e.Id, $what[$e.Id])) }
    $logs = Get-WinEvent -ListLog 'Microsoft-Surface-*' -ErrorAction SilentlyContinue | Where-Object { $_.RecordCount }
    foreach ($l in $logs) {
        $short = $l.LogName -replace '^Microsoft-Surface-|/Operational$', ''
        foreach ($e in Get-WinEvent -FilterHashtable @{ LogName = $l.LogName; StartTime = $since } -ErrorAction SilentlyContinue) {
            $t = ([DateTimeOffset]$e.TimeCreated).ToUnixTimeMilliseconds() / 1000
            if ($e.Message -match 'Channel Index=(\d+), ChannelInstance=(\d+), Value=(\d+)') {
                $out.Add(('{0},{1},{2},SMF ch{3}/{4},{5}' -f $t, $short, $e.Id, $Matches[1], $Matches[2], $Matches[3]))
            } else {
                $m = ($e.Message -split "`r?`n")[0] -replace '[,]', ';'
                if ($m.Length -gt 90) { $m = $m.Substring(0, 90) }
                $out.Add(('{0},{1},{2},{3},' -f $t, $short, $e.Id, $m))
            }
        }
    }
    [IO.File]::WriteAllLines((Join-Path $dir 'winevents.csv'), $out, $Utf8)
}

# ---- snapshots
function Save-Info([string]$name, [string]$phase) {
    $d = Join-Path $script:Raw 'info'; New-Item -ItemType Directory -Force $d | Out-Null
    $f = Join-Path $d "$name-$phase.txt"
    $txt = switch ($name) {
        'display' { & $Kit.Probe display }
        'pointerdevs' { & $Kit.Probe devices }
        'hidcaps-ble' {
            if ($NoElevated) { '(elevation skipped)' } else {
                & powershell -NoProfile -ExecutionPolicy Bypass -File $Kit.Elev (Join-Path $Kit.Repo 'tools\windows\hid-caps.ps1') -Filter '0c0f' 2>&1 | ForEach-Object { "$_" }
            }
        }
        'camlist' {
            $ff = Get-Ffmpeg
            foreach ($c in 'Front', 'Rear') {
                "dshow Surface Camera ${c}:"
                if ($ff) { & $ff -hide_banner -list_options true -f dshow -i "video=Surface Camera $c" 2>&1 | ForEach-Object { "$_" } | Select-String 'pixel_format=nv12' | ForEach-Object { '  ' + ($_.Line -replace '^\[.*?\]\s*', '') } }
            }
            'WinRT frame sources:'
            & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Kit.Dir 'camera-sources.ps1') 2>&1 | ForEach-Object { "$_" } |
                Select-String -Pattern '^group|open kind=|open failed|^\s+\d+x\d+' | ForEach-Object { $_.Line }
        }
        'audiodev' { & $Kit.Probe volume list; Get-PnpDevice -PresentOnly -Class AudioEndpoint -ErrorAction SilentlyContinue | ForEach-Object { "endpoint: $($_.FriendlyName) [$($_.Status)]" } }
        'btdevs' { Get-PnpDevice -PresentOnly -Class Bluetooth -ErrorAction SilentlyContinue | Sort-Object FriendlyName | ForEach-Object { "$($_.Status) $($_.FriendlyName)" } | Select-Object -Unique }
        'wlan' {
            # netsh wlan needs Location services (a setting this kit does not change); fall back to the adapter link speeds.
            # The \uXXXX alternatives are the same labels on a Korean-language Windows.
            $w = netsh wlan show interfaces 2>$null | Where-Object { $_ -match '^\s*(Description|State|Radio type|Band|Channel|Receive rate|Transmit rate|Signal|Authentication|Cipher|\uC124\uBA85|\uC0C1\uD0DC|\uB77C\uB514\uC624 \uC885\uB958|\uB300\uC5ED|\uCC44\uB110|\uC218\uC2E0 \uC18D\uB3C4|\uC804\uC1A1 \uC18D\uB3C4|\uC2E0\uD638|\uC778\uC99D)\s*:' } | ForEach-Object { $_.Trim() }
            if ($w) { $w } else {
                '(netsh wlan: the location service is off, so band, channel and signal cannot be read. Adapter link speeds only)'
                Get-NetAdapter -Physical -ErrorAction SilentlyContinue | Where-Object { $_.MediaType -match '802.11' -or $_.PhysicalMediaType -match '802.11' } | ForEach-Object {
                    "adapter: $($_.InterfaceDescription), status $($_.Status), link rx $([math]::Round($_.ReceiveLinkSpeed / 1e6)) / tx $([math]::Round($_.TransmitLinkSpeed / 1e6)) Mbit/s, driver $($_.DriverVersion)"
                }
            }
            # driver capabilities (no location permission needed) and the signal bars of the connected profile (profile name not printed)
            netsh wlan show drivers 2>$null | Where-Object { $_ -match '^\s*(Driver|Version|Radio types supported|\uB4DC\uB77C\uC774\uBC84|\uBC84\uC804|\uC9C0\uC6D0\uB418\uB294 \uBB34\uC120 \uC720\uD615)\s*:' } | ForEach-Object { $_.Trim() }
            try {
                $null = [Windows.Networking.Connectivity.NetworkInformation, Windows.Networking.Connectivity, ContentType = WindowsRuntime]
                $cp = [Windows.Networking.Connectivity.NetworkInformation]::GetInternetConnectionProfile()
                if ($cp -and $cp.IsWlanConnectionProfile) { "signal bars: $($cp.GetSignalBars()) / 5" }
            } catch {}
        }
        'brightness' {
            $m = Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorBrightness -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($m) {
                $lv = @($m.Level)
                "current brightness $($m.CurrentBrightness)%, $($m.Levels) levels ($(($lv | Measure-Object -Minimum).Minimum)..$(($lv | Measure-Object -Maximum).Maximum))"
            }
        }
        'sam-events' { & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Kit.Repo 'tools\windows\sam-events.ps1') }
        'penbattery' {
            $pen = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -match 'Slim Pen' } | Select-Object -First 1
            if ($pen) {
                $b = (Get-PnpDeviceProperty -InstanceId $pen.InstanceId -KeyName '{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2' -ErrorAction SilentlyContinue).Data
                "pen battery (PnP {104EA319…} 2): $b %"
                $hid = @(Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -match '^HID\\.*0C0F' })
                "pen BLE HID collections: " + (($hid | ForEach-Object { "$($_.FriendlyName)=$($_.Status)" }) -join ', ')
            } else { 'pen not paired' }
        }
    }
    $lines = @(@($txt) | Where-Object { $null -ne $_ } | ForEach-Object { Protect-Text "$_" })
    if (-not $lines) { $lines = @("($name`: no output)") }
    try { [IO.File]::WriteAllLines($f, [string[]]$lines, $Utf8) } catch { Say "  saving snapshot $name failed: $($_.Exception.Message)" 'DarkYellow' }
}

# ---- actions (a step's Run)
function Invoke-Stress([int]$sec) {
    if ($DryRun) { $sec = $StepSeconds }
    Say "  load ${sec} s ($env:NUMBER_OF_PROCESSORS threads)" 'DarkGray'
    & $Kit.Probe stress $sec | Out-Null
}

function Invoke-Audio([string]$cmd, [int]$volume, [string]$extra = '', [string]$tag = '') {
    if ($DryRun -and -not $AllowSound) { Say "  (dry-run: step with sound skipped: $cmd)" 'DarkYellow'; return }
    $d = Join-Path $script:Raw 'audio'; New-Item -ItemType Directory -Force $d | Out-Null
    if ($cmd -ne 'record') {
        Say "  playing at speaker volume ${volume}%" 'DarkGray'
        & $Kit.Probe volume set Omnisonic ($volume / 100).ToString($Inv) 0 | Out-Null
    }
    $a = @('-I', '-X', 'utf8', (Join-Path $Kit.Dir 'audio.py'), $cmd, '--out', $d) + ($extra -split ' ' | Where-Object { $_ })
    if ($tag) { $a += @('--tag', $tag) }
    & $Kit.Python @a | Out-Null
}

function Invoke-Cam([string]$cam, [string]$size, [int]$fps, [int]$sec, [string]$Tag = '', [switch]$Show, [switch]$Sharp) {
    $ff = Get-Ffmpeg
    $d = Join-Path $script:Raw 'cam'; New-Item -ItemType Directory -Force $d | Out-Null
    if (-not $ff) { [IO.File]::AppendAllText((Join-Path $d 'capture.txt'), "ffmpeg missing`n", $Utf8); return }
    $name = "$cam-$size" + $(if ($Tag) { "-$Tag" })
    $jpg = Join-Path $d "$name.jpg"
    $log = & $ff -hide_banner -f dshow -video_size $size -framerate $fps -i "video=Surface Camera $cam" -t $sec -vf showinfo -f null NUL -frames:v 1 -update 1 -y $jpg 2>&1 | ForEach-Object { "$_" }
    $pts = @($log | Select-String 'pts_time:([\d.]+)' | ForEach-Object { [double]$_.Matches[0].Groups[1].Value })
    $fmt = ($log | Select-String 'Stream #0:0: Video: (.*)' | Select-Object -First 1 | ForEach-Object { $_.Matches[0].Groups[1].Value }) -replace ', start [\d.]+', ''
    $line = if ($pts.Count -gt 2) {
        "$cam $size@$fps requested: $($pts.Count) frames, measured {0:F1} fps, format $fmt" -f (($pts.Count - 1) / ($pts[-1] - $pts[0]))
    } else {
        $err = ($log | Select-String -Pattern 'error|could not|failed' | Select-Object -First 2 | ForEach-Object { $_.Line }) -join ' / '
        "$cam $size@$fps requested: no frames ($err)"
    }
    if ($Sharp -and (Test-Path $jpg)) {
        $pgm = Join-Path $d "$name.pgm"
        & $ff -hide_banner -loglevel error -y -i $jpg -pix_fmt gray $pgm 2>$null
        $s = & $Kit.Python -I -c "import numpy as n,sys;b=open(sys.argv[1],'rb').read();p=b.split(maxsplit=4);w,h=int(p[1]),int(p[2]);a=n.frombuffer(p[4][-w*h:],n.uint8).reshape(h,w).astype(float);l=a[1:-1,1:-1]*4-a[:-2,1:-1]-a[2:,1:-1]-a[1:-1,:-2]-a[1:-1,2:];print(round(l.var(),1))" $pgm
        $line += ", sharpness (Laplacian variance) $s"
    }
    Say "  $line" 'DarkGray'
    [IO.File]::AppendAllText((Join-Path $d 'capture.txt'), "$line`n", $Utf8)
    [IO.File]::AppendAllText((Join-Path $d 'value.val'), "$($line -replace ' requested:', ':' -replace ', format.*?(?=, sharpness|$)', ''); ", $Utf8)
    if ($Show -and -not $DryRun -and (Test-Path $jpg)) { Invoke-Item $jpg }
}

function Invoke-CamAll {
    $ff = Get-Ffmpeg
    if (-not $ff) { return }
    foreach ($c in 'Front', 'Rear') {
        $modes = & $ff -hide_banner -list_options true -f dshow -i "video=Surface Camera $c" 2>&1 | ForEach-Object { "$_" } |
            Select-String 'pixel_format=nv12\s+min s=(\d+x\d+) fps=\S+ max s=\S+ fps=(\d+)' | ForEach-Object { , @($_.Matches[0].Groups[1].Value, [int]$_.Matches[0].Groups[2].Value) }
        foreach ($m in $modes) { Invoke-Cam $c $m[0] $m[1] 2 }
    }
}

function Invoke-Iperf {
    $d = Join-Path $script:Raw 'net'; New-Item -ItemType Directory -Force $d | Out-Null
    $ip = Get-Command iperf3 -ErrorAction SilentlyContinue
    if (-not $ip -or $DryRun) { [IO.File]::WriteAllText((Join-Path $d 'iperf.txt'), "iperf3 skipped (not installed or dry-run)`n", $Utf8); return }
    $srv = Read-Host 'iperf3 server address (Enter skips)'
    if (-not $srv) { return }
    $res = foreach ($rev in @($false, $true)) {
        $a = @('-c', $srv, '-t', '10', '-J'); if ($rev) { $a += '-R' }
        $j = & $ip.Source @a 2>$null | ConvertFrom-Json
        $bps = $j.end.sum_received.bits_per_second
        "{0} {1:F0} Mbit/s" -f $(if ($rev) { 'receive' } else { 'send' }), ($bps / 1e6)
    }
    [IO.File]::WriteAllLines((Join-Path $d 'iperf.txt'), [string[]]$res, $Utf8)
    [IO.File]::WriteAllText((Join-Path $d 'value.val'), ($res -join ', '), $Utf8)
}

# ---- pen preparation
function Test-PenAwake {
    $hid = @(Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -match '^HID\\.*0C0F' -and $_.Status -eq 'OK' })
    return $hid.Count -gt 0
}

# ---------------------------------------------------------------- result recording
function Write-Result($t, [string]$result, [string]$basis, [string]$setting, [string]$memo, [string]$summary, [string[]]$stepTexts) {
    New-Item -ItemType Directory -Force $Kit.Public | Out-Null
    $build = Get-WindowsBuild
    $res = Join-Path $Kit.Public 'results.md'
    if (-not (Test-Path $res)) {
        $head = @(
            '# Windows baseline test results', '',
            'Windows results per ID of [docs/test-plan.md](../../../docs/test-plan.md). The format is that of `docs/test-results.md`; the "Kernel" column holds the Windows build.',
            'The collection summary per ID is `<ID>.md` in this folder; the tool is [tools/windows/baseline](../../../tools/windows/baseline/README.md). Running an ID again adds a row.', '',
            '| ID | Result | Evidence | Kernel | Config | Values, notes |', '|---|---|---|---|---|---|')
        [IO.File]::WriteAllLines($res, [string[]]$head, $Utf8)
    }
    $esc = { param($s) (Protect-Text $s) -replace '\|', '/' -replace '[\r\n]+', ' ' }
    [IO.File]::AppendAllText($res, ("| {0} | {1} | {2} | {3} | {4} | {5} |`n" -f $t.Id, $result, $basis, $build, (& $esc $setting), (& $esc $memo)), $Utf8)

    $md = New-Object Collections.Generic.List[string]
    $md.Add("# $($t.Id) $($t.Name) (Windows baseline)"); $md.Add('')
    $md.Add("| Item | Value |"); $md.Add('|---|---|')
    $md.Add("| Windows | $build |"); $md.Add("| Config | $(& $esc $setting) |"); $md.Add("| Result | $result ($basis) |"); $md.Add("| Values, notes | $(& $esc $memo) |")
    $md.Add("| Linux side | [docs/test-plan.md](../../../docs/test-plan.md) $($t.Id) |"); $md.Add('')
    $md.Add('## Steps'); $md.Add('')
    $i = 1; foreach ($s in $stepTexts) { $md.Add("$i. $(($s -replace '^\[clear\]\s*', ''))"); $i++ }
    $md.Add(''); $md.Add('## Collection summary'); $md.Add('')
    foreach ($l in ($summary -split "`n")) { $md.Add((Protect-Text $l)) }
    [IO.File]::WriteAllLines((Join-Path $Kit.Public "$($t.Id).md"), $md, $Utf8)
}

# ---------------------------------------------------------------- run
function Invoke-Test($t) {
    Say ''
    Say ('=' * 70)
    Say "$($t.Id)  $($t.Name)" 'White'
    if ($Auto) {
        $why = Get-AutoSkip $t
        if ($why) { Say "  skipped: $why" 'DarkYellow'; $script:AutoLog += , @($t.Id, 'skipped', $why); return }
        # the automatic part replaces the steps; captures and snapshots come from the test unless the plan overrides them
        $p = $AutoPlan[$t.Id]
        $t = $t.Clone()
        $t.Steps = $p.Steps
        if ($p.ContainsKey('Caps')) { $t.Caps = $p.Caps }
        if ($p.ContainsKey('Info')) { $t.Info = $p.Info }
        if ($p.Setting) { $t.Setting = $p.Setting }
        $t.Human = $p.Human
        $t.Do = "unattended: " + ((@($p.Steps) | ForEach-Object { $_.Say }) -join ' → ')
    }
    if ($t.Na) { Say "  no Windows counterpart: $($t.Na)" 'DarkYellow'; return }
    if ($t.Needs -and -not $Auto) { Say "  needs: $($t.Needs)" }
    Say "  $($t.Do)" 'Gray'
    if ($t.Sound -and $Auto) { Say '  caution: plays a quiet test signal on the internal speakers (volume 20%, -24 dBFS). The volume is restored afterwards.' 'Yellow' }
    elseif ($t.Sound -and -not $DryRun) { Say '  caution: plays a test signal on the internal speakers (-18 dBFS, at a set volume). The volume is restored afterwards.' 'Yellow' }
    if ($t.Elevated -and -not $NoElevated) { Say '  part of the collection runs elevated (run-elevated.ps1).' 'DarkGray' }

    # prerequisites
    $missing = @()
    if (-not (Test-Path $Kit.Probe)) { $missing += 'sp9probe.exe' }
    if (-not (Test-Path $Kit.Python)) { $missing += 'Python venv' }
    if ($missing) { Say "  not ready: $($missing -join ', ') → setup.ps1" 'Red'; if (-not $DryRun) { return } }
    if ($t.Needs -match 'pen \(awake\)' -and -not $DryRun -and -not $Auto) {
        while (-not (Test-PenAwake)) {
            Say '  pen BLE HID not present (asleep). Wake the pen with the tail button or by pressing it on the screen, then Enter (s skips).' 'Yellow'
            if ((Read-Host) -eq 's') { break }
        }
    }
    if (-not $DryRun -and -not $Auto -and -not $t.Manual) { if ((Read-Host '  Enter to start (s skips)') -eq 's') { return } }

    $stamp = (Get-Date -Format 'yyyyMMdd-HHmmss') + $(if ($Auto) { '-auto' })
    $base = if ($DryRun) { Join-Path $Kit.Private "_dryrun\$($t.Id)" } else { Join-Path $Kit.Private $t.Id }
    $script:Raw = Join-Path $base $stamp
    New-Item -ItemType Directory -Force $script:Raw | Out-Null
    $script:T0 = Get-Date
    $vol = $null
    if ($t.Sound) { $vol = (& $Kit.Probe volume get Omnisonic) -split ' ' }
    $stepTexts = @()
    try {
        foreach ($i in @($t.Info)) { if ($i) { Save-Info $i 'start' } }
        if (-not $t.Manual) { Start-Captures $t }
        $n = 0
        foreach ($s in $t.Steps) {
            $n++
            $say = if ($s -is [hashtable]) { $s.Say } else { $s }
            $stepTexts += $say
            Add-Marker $n $say
            if ($script:NextFile) { [IO.File]::WriteAllText((Join-Path (Split-Path $script:NextFile) 'step.txt'), "[$($t.Id) $n/$(@($t.Steps).Count)] $say", $Utf8) }
            Say "  [$n/$(@($t.Steps).Count)] $($say -replace '^\[clear\]\s*', '')" 'White'
            if ($s -is [hashtable] -and $s.Run) { & $s.Run }
            else {
                $r = Wait-Next $(if ($t.Timeout) { $t.Timeout } else { 900 })
                if ($r -eq 'timeout') { Say '  (timed out, next step)' 'DarkYellow'; Add-Marker "$n-timeout" 'timeout' }
            }
        }
        Add-Marker 'end' 'end'
    } finally {
        if (-not $t.Manual) { Stop-Captures $t }
        foreach ($i in @($t.Info)) { if ($i) { Save-Info $i 'end' } }
        if ($vol) { & $Kit.Probe volume set Omnisonic $vol[0] $vol[1] | Out-Null; Say "  speaker volume restored to $([math]::Round([double]::Parse($vol[0], $Inv) * 100))%" 'DarkGray' }
    }

    # summary
    $sum = & $Kit.Python -I -X utf8 (Join-Path $Kit.Dir 'summarize.py') --test $t.Id --raw $script:Raw 2>&1 | ForEach-Object { "$_" }
    $value = ($sum | Where-Object { $_ -like 'VALUE: *' } | Select-Object -Last 1) -replace '^VALUE: ', ''
    $body = ($sum | Where-Object { $_ -notlike 'VALUE: *' }) -join "`n"
    Say ''
    Write-Host (Protect-Text $body)
    Say "  raw output: $script:Raw" 'DarkGray'
    if ($DryRun) { Say "  suggested value: $(Protect-Text $value)" 'Green'; return }

    if ($Auto) {
        $result = if ($value) { 'measured' } else { 'unverified' }
        $memo = $value
        if ($t.Human) { $memo = (@($value, "needs checking: $($t.Human)") | Where-Object { $_ }) -join '; ' }
        $setting = if ($t.Setting) { $t.Setting } else { 'default' }
        $steps = @($stepTexts) + @("(unattended run, -Auto. Needs checking: $(if ($t.Human) { $t.Human } else { 'nothing' }))")
        Write-Result $t $result 'measured (unattended)' $setting $memo $body $steps
        $script:AutoLog += , @($t.Id, $result, $(if ($t.Human) { $t.Human } else { '' }))
        Say "  written: hw/windows/baseline/results.md, hw/windows/baseline/$($t.Id).md" 'Green'
        return
    }

    # ask
    Say ''
    $r = Read-Host '  Result? 1 pass / 2 partial / 3 fail / 4 unverified'
    $result = @{ '1' = 'pass'; '2' = 'partial'; '3' = 'fail'; '4' = 'unverified' }[$r]; if (-not $result) { $result = 'unverified' }
    $defBasis = if ($value -and -not $t.Manual) { 'measured' } else { 'observed' }
    $b = Read-Host "  Evidence? 1 measured / 2 observed (Enter = $defBasis)"
    $basis = @{ '1' = 'measured'; '2' = 'observed' }[$b]; if (-not $basis) { $basis = $defBasis }
    $defSet = if ($t.Setting) { $t.Setting } else { 'default' }
    $setting = Read-Host "  Config? (Enter = $defSet)"; if (-not $setting) { $setting = $defSet }
    Say "  suggested value: $(Protect-Text $value)" 'Green'
    $memo = Read-Host '  Values, notes? Enter = the suggestion as is, +text = appended to the suggestion, anything else = replaces it'
    if (-not $memo) { $memo = $value } elseif ($memo.StartsWith('+')) { $memo = ($value + '; ' + $memo.Substring(1)).Trim('; ') }
    Write-Result $t $result $basis $setting $memo $body $stepTexts
    Say "  written: hw/windows/baseline/results.md, hw/windows/baseline/$($t.Id).md" 'Green'
}

if ($List -or (-not $Test -and -not $Area -and -not $Auto)) {
    $area = ''
    foreach ($t in $Tests) {
        $a = $t.Id.Split('-')[0]
        if ($a -ne $area) { Write-Host ''; $area = $a }
        $tag = if ($t.Na) { '[no counterpart] ' + $t.Na } elseif ($t.Manual) { '[observation only] ' + $t.Do } else { $t.Do }
        $flags = @(); if ($t.Elevated) { $flags += 'elevated' }; if ($t.Sound) { $flags += 'sound' }; if ($t.Needs) { $flags += $t.Needs }
        Write-Host ("{0,-8} {1}" -f $t.Id, $t.Name) -NoNewline -ForegroundColor White
        Write-Host $(if ($flags) { "  ($($flags -join ', '))" } else { '' }) -ForegroundColor DarkGray
        Write-Host ("         {0}" -f $tag)
    }
    return
}

# Rebuild the probe when its source is newer (setup.ps1 is idempotent)
$src = Join-Path $Kit.Dir 'sp9probe.cs'
if (-not (Test-Path $Kit.Probe) -or -not (Test-Path $Kit.Python) -or (Get-Item $src).LastWriteTime -gt (Get-Item $Kit.Probe).LastWriteTime) {
    Say 'running setup.ps1 (build, venv)' 'DarkGray'
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Kit.Dir 'setup.ps1') | ForEach-Object { "  $_" } | Write-Host
}

$sel = @()
if ($Test) { $ids = $Test | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim().ToUpper() } | Where-Object { $_ }; $sel = $Tests | Where-Object { $ids -contains $_.Id } }
if ($Area) { $sel += $Tests | Where-Object { $_.Id -like "$($Area.ToUpper())-*" } }
if ($Auto -and -not $Test -and -not $Area) { $sel = $Tests }   # -Auto alone: every test, the manual ones skipped
if (-not $sel) { Say 'no such ID. See -List' 'Red'; exit 1 }
$script:AutoLog = @()
foreach ($t in $sel) { Invoke-Test $t }
if ($Auto) {
    Say ''
    Say ('=' * 70)
    Say 'Unattended run summary' 'White'
    foreach ($r in $script:AutoLog) { Write-Host ("  {0,-8} {1,-10} {2}" -f $r[0], $r[1], $r[2]) }
}
