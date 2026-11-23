# Copyright 2010 New Relic, Inc.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

import os
import sys

INSTRUMENTATION_PATH = os.path.dirname(__file__)
INSTRUMENTATION_VENDOR_PATH = os.path.join(INSTRUMENTATION_PATH, "vendor")
SDIST_PATH = str(os.path.join(INSTRUMENTATION_PATH, "newrelic"))


def get_supported_tags():
    """
    Load pip and run compatibility_tags.get_supported(). If pip is not present
    or does not contain this function, load our vendored version of pip instead.
    """
    try:
        # Attempt to use any existing installs of pip to find supported tags
        from pip._internal.utils.compatibility_tags import get_supported

        return get_supported()
    except Exception:
        pass

    # Unable to find pip, or pip version does not have compatibility tags
    if "pip" in sys.modules:
        # Save original pip module and load our own copy
        original_pip = sys.modules["pip"]
        del sys.modules["pip"]
    else:
        original_pip = None

    try:
        # Insert our vendored version of pip into the path, and try to return
        # the supported tags again.
        sys.path.insert(0, INSTRUMENTATION_VENDOR_PATH)
        from pip._internal.utils.compatibility_tags import get_supported

        return get_supported()
    finally:
        # Remove our vendored version of pip from the path
        if INSTRUMENTATION_VENDOR_PATH in sys.path:
            del sys.path[sys.path.index(INSTRUMENTATION_VENDOR_PATH)]

        # Replace original pip module for compatibility
        if original_pip:
            sys.modules["pip"] = original_pip
        elif "pip" in sys.modules:
            del sys.modules["pip"]


def find_supported_newrelic_distribution():
    """
    Return the path to a supported wheel or sdist included under INSTRUMENTATION_PATH by the Python agent init container.
    """
    try:
        wheels = list(os.listdir(INSTRUMENTATION_PATH))
        for tag in get_supported_tags():
            tag = str(tag)
            for wheel in wheels:
                if tag in wheel:
                    return str(os.path.join(INSTRUMENTATION_PATH, wheel))
    except Exception:
        pass

    return SDIST_PATH
