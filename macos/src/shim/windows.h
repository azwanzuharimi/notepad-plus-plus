// SPDX-License-Identifier: GPL-3.0-or-later
// LexUser.cxx includes windows.h only for _itoa; this header gives that function on macOS.
#pragma once
#include <cstdio>

static inline char *_itoa(int value, char *buf, int /*radix*/) {
	std::snprintf(buf, 12, "%d", value);
	return buf;
}
