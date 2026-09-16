# Third-party notices

This plugin is licensed under the MIT License. It includes third-party software
whose licenses require notices in addition to the MIT and Apache-2.0 notices of
its other dependencies. Those notices are reproduced below.

This file is not a complete attribution set for the plugin's dependencies.

## jpeg-encoder

   * Version: 0.7.1
   * License: `(MIT OR Apache-2.0) AND IJG`
   * Repository: <https://github.com/vstroebel/jpeg-encoder>

Portions of this software are derived from the Independent JPEG Group's
software, copyright (C) 1991-2020, Thomas G. Lane, Guido Vollbeding. Those
portions are licensed under the IJG License in addition to MIT or Apache-2.0.

Where only executable code is distributed, the accompanying documentation must
state:

```text
this software is based in part on the work of the Independent JPEG Group
```

Where source code is distributed, the IJG README must be included with its
copyright and no-warranty notice unaltered, and any changes to the original
files must be clearly indicated in accompanying documentation.

The IJG software is provided without warranty, and its authors accept no
liability for damages of any kind. The names of the IJG authors may not be used
in advertising or publicity relating to this software or products derived from
it.

The full license text is included in the `jpeg-encoder` crate as `LICENSE-IJG`.

## openh264

   * Version: 0.9.7, via `openh264-sys2` 0.9.7
   * License: `BSD-2-Clause`
   * Repository: <https://github.com/ralfbiedert/openh264-rs>
   * Includes: Cisco OpenH264, <https://github.com/cisco/openh264>

Redistributions in binary form must reproduce the following notice in the
documentation and/or other materials provided with the distribution:

```text
Copyright (c) 2013, Cisco Systems
All rights reserved.

Redistribution and use in source and binary forms, with or without modification,
are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice, this
  list of conditions and the following disclaimer in the documentation and/or
  other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### Patent notice

The BSD-2-Clause license grants copyright permissions only and grants no patent
rights. H.264/AVC is subject to patents. Cisco's payment of AVC patent pool
royalties applies only to OpenH264 binaries distributed by Cisco, and does not
extend to OpenH264 compiled from source, as it is in this plugin. Responsibility
for any patent licensing rests with the party distributing the application.

## Distributing applications

These notices apply to any application distributed in binary form that includes
this plugin. The distributor of that application is responsible for:

   * Providing attribution for all third-party software included in the
     application, including the notices in this file.
   * Delivering those notices with the application, in its documentation or
     other materials provided with the distribution.
   * Determining whether H.264/AVC patent licensing is required.
