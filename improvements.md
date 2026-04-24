# Improving Your WiX Installer UX (bloom)

This guide walks through **practical, modern UX improvements** for your WiX-based installer, along with **how to implement each one**. The goal is to reduce user friction, simplify decisions, and make your installer feel like a polished product rather than a legacy MSI.

---

# 1. Replace the Feature Tree UI

## Problem

`WixUI_FeatureTree` exposes too many technical options:

* Users don’t understand format categories
* Too many decisions during install

## Solution

Switch to a simpler UI:

```xml
<UIRef Id="WixUI_InstallDir"/>
```

## Result

* Cleaner flow
* Users only choose install location
* No overwhelming feature tree

---

# 2. Replace Feature Tree with Simple Options

## Goal

Replace dozens of checkboxes with **one meaningful choice**

### Add a property:

```xml
<Property Id="INSTALL_ASSOCIATIONS" Value="1"/>
```

### Add a checkbox to UI (requires custom dialog or Publish logic)

Conceptually:

* ✅ Set bloom as default image viewer

---

## Condition Components

Wrap association components:

```xml
<Component Id="AssocCommon" Guid="*">
    <Condition>INSTALL_ASSOCIATIONS=1</Condition>
    ...
</Component>
```

---

## Result

* Default = works out of the box
* Advanced users can opt out
* No confusion about formats

---

# 3. Default Behavior Strategy

## Recommended Defaults

| Feature                          | Default |
| -------------------------------- | ------- |
| Common formats (jpg, png, gif)   | ✅ ON    |
| Modern formats (avif, jxl, heic) | ❌ OFF   |
| RAW formats                      | ❌ OFF   |
| Specialty formats                | ❌ OFF   |

---

## Implementation

Split behavior:

```xml
<Condition>INSTALL_ASSOCIATIONS=1 AND INSTALL_ADVANCED=1</Condition>
```

Or:

* Only install `AssocCommon` by default
* Gate others behind advanced mode

---

# 4. Add Branding (Huge UX Upgrade)

## Add Images

```xml
<WixVariable Id="WixUIBannerBmp" Value="assets\ui\banner.bmp"/>
<WixVariable Id="WixUIDialogBmp" Value="assets\ui\dialog.bmp"/>
```

## Image Specs

| Type   | Size    |
| ------ | ------- |
| Banner | 493x58  |
| Dialog | 493x312 |

---

## Result

* Feels like a real product
* Matches your app branding
* Immediate perceived quality boost

---

# 5. Fix Installer Flow

## Problem

You force users into customization:

```xml
<Publish Dialog="WelcomeDlg" Control="Next" Event="NewDialog" Value="CustomizeDlg"/>
```

## Fix

Remove that override.

### Default Flow:

1. Welcome
2. Install (recommended)
3. Optional Customize

---

## Result

* Faster installs
* Matches user expectations

---

# 6. File Associations (Windows 10/11 Reality)

## Problem

Writing to:

```
HKLM\Software\Classes\.jpg
```

does NOT guarantee:

* Default app selection
* User-visible behavior

---

## Correct Approach

### Keep:

* ProgID (`bloom.AssocFile`)
* `OpenWithProgids`

### Add (optional but better):

```xml
<RegistryValue Root="HKLM"
               Key="Software\RegisteredApplications"
               Name="bloom"
               Value="Software\bloom\Capabilities"/>
```

---

### Capabilities Example

```xml
<RegistryKey Root="HKLM" Key="Software\bloom\Capabilities">
    <RegistryValue Name="ApplicationName" Value="bloom"/>
    <RegistryValue Name="ApplicationDescription" Value="Image Viewer"/>
</RegistryKey>

<RegistryKey Root="HKLM" Key="Software\bloom\Capabilities\FileAssociations">
    <RegistryValue Name=".jpg" Value="bloom.AssocFile"/>
</RegistryKey>
```

---

## Result

* Windows recognizes your app properly
* Appears in “Default Apps”
* More future-proof

---

# 7. PATH Modification (Make Optional)

## Problem

Currently:

```xml
<System="yes"/>
```

* Modifies system PATH
* Most users don’t need it

---

## Fix

### Add property:

```xml
<Property Id="ADD_TO_PATH" Value="0"/>
```

### Add condition:

```xml
<Component Id="Path" Guid="..." KeyPath="yes">
    <Condition>ADD_TO_PATH=1</Condition>
```

---

## UI Option

Checkbox:

* ☐ Add bloom to PATH (advanced)

---

## Result

* Cleaner installs
* Avoids unnecessary system changes

---

# 8. Launch App After Install

## Add Custom Action

```xml
<CustomAction Id="LaunchApplication"
              FileKey="exe0"
              Execute="immediate"
              Return="asyncNoWait"/>
```

---

## Hook into Exit Dialog

```xml
<Publish Dialog="ExitDialog"
         Control="Finish"
         Event="DoAction"
         Value="LaunchApplication">1</Publish>
```

---

## Optional Checkbox

```xml
<Property Id="LAUNCHAPP" Value="1"/>
```

Condition:

```xml
<Publish ...>LAUNCHAPP=1</Publish>
```

---

## Result

* Immediate feedback
* Better first-run experience

---

# 9. Improve Uninstall Cleanliness

## Already Good

* Start menu cleanup ✔️

## Add Consideration

* Only remove associations you installed
* Avoid overriding other apps

---

## Optional Cleanup

Track ownership via registry key:

```xml
HKLM\Software\bloom\InstalledAssociations
```

---

# 10. Add ARP Metadata

## Improves Control Panel / Settings listing

```xml
<Property Id="ARPCOMMENTS" Value="Fast modern image viewer"/>
<Property Id="ARPURLINFOABOUT" Value="https://your-site.com"/>
<Property Id="ARPHELPLINK" Value="https://your-site.com/help"/>
```

---

## Result

* More professional listing
* Better user trust

---

# 11. Rename Internal Directory (Minor)

## Current

```xml
Name="PFiles"
```

## Better

```xml
Name="Program Files"
```

(Not user-facing, but cleaner)

---

# 12. Optional: Move Beyond Classic MSI

## If you want modern UX:

### Options

* WiX Burn (bootstrapper with custom UI)
* MSIX packaging (modern Windows apps)
* Custom installer frontend

---

## Tradeoffs

| Option        | UX        | Complexity  |
| ------------- | --------- | ----------- |
| MSI (current) | Medium    | Low         |
| Burn          | High      | Medium      |
| MSIX          | Very High | Medium/High |

---

# Final Recommended Setup (Minimal Changes, Max Impact)

If you only do a few things, do these:

### MUST DO

* Switch to `WixUI_InstallDir`
* Add branding images
* Remove forced Customize dialog

### SHOULD DO

* Collapse associations into 1 checkbox
* Default only common formats
* Make PATH optional

### NICE TO HAVE

* Launch after install
* Add ARP metadata
* Register capabilities

---

# Summary

Your installer is already **technically strong**, but:

* Too many choices → simplify
* Too much exposure → hide complexity
* Too generic → add branding

With these changes, your installer will feel:

* Faster
* Cleaner
* More modern
* More aligned with user expectations

---

If you want, I can generate a **fully rewritten WiX file** implementing all of this cleanly.

