// SPDX-License-Identifier: GPL-3.0-or-later
#include <cstring>
#include <memory>
#include "uchardet.h"

// Runs uchardet; on an exception the result is empty.
extern "C" void npp_detect_charset(const char *data, size_t len, char *out, size_t outLen) {
	out[0] = '\0';
	try {
		std::unique_ptr<void, void (*)(uchardet_t)> ud(uchardet_new(), uchardet_delete);
		uchardet_handle_data(ud.get(), data, len);
		uchardet_data_end(ud.get());
		strncpy(out, uchardet_get_charset(ud.get()), outLen - 1);
		out[outLen - 1] = '\0';
	} catch (...) {
		out[0] = '\0';
	}
}
