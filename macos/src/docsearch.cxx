// SPDX-License-Identifier: GPL-3.0-or-later
// C entry points to search a Scintilla Document with the Boost regex engine, also off the main thread.
#include <cstdint>
#include <cstring>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

#include "ScintillaTypes.h"
#include "ScintillaMessages.h"
#include "ILoader.h"
#include "ILexer.h"
#include "Scintilla.h"
#include "Debugging.h"
#include "Geometry.h"
#include "Platform.h"
#include "Position.h"
#include "SplitVector.h"
#include "Partitioning.h"
#include "RunStyles.h"
#include "CellBuffer.h"
#include "CharClassify.h"
#include "Decoration.h"
#include "CaseFolder.h"
#include "CharacterCategoryMap.h"
#include "Document.h"
#include "BoostRegexSearch.h"

using namespace Scintilla::Internal;

static std::mutex searchLock;
static thread_local std::string lastError;

struct Markings {
	std::vector<SearchResultMarkingLine> lines;
	SearchResultMarkings s{0, nullptr};
};

extern "C" {

Document *npp_doc_new(const char *text, intptr_t len) {
	Document *d = nullptr;
	try {
		d = new Document(Scintilla::DocumentOption::Default);
		d->AddRef();
		d->SetDBCSCodePage(SC_CP_UTF8);
		d->InsertString(0, text, len);
		return d;
	} catch (std::exception &) {
		if (d)
			d->Release();
		return nullptr;
	}
}

void npp_doc_free(Document *d) { d->Release(); }

Document *npp_doc_from_pointer(void *p) {
	return static_cast<Document *>(static_cast<Scintilla::IDocumentEditable *>(p));
}

intptr_t npp_doc_length(Document *d) { return d->Length(); }

void npp_doc_text(Document *d, char *buf, intptr_t pos, intptr_t len) { d->GetCharRange(buf, pos, len); }

int npp_doc_char_at(Document *d, intptr_t pos) { return static_cast<unsigned char>(d->CharAt(pos)); }

intptr_t npp_doc_line_from_pos(Document *d, intptr_t pos) { return d->SciLineFromPosition(pos); }
intptr_t npp_doc_line_start(Document *d, intptr_t line) { return d->LineStart(line); }
intptr_t npp_doc_line_end(Document *d, intptr_t line) { return d->LineEnd(line); }
intptr_t npp_doc_lines(Document *d) { return d->LinesTotal(); }

// Returns the match position, -1 if none, -2 or -3 for a regex error (see npp_regex_error).
intptr_t npp_doc_find(Document *d, intptr_t minPos, intptr_t maxPos, const char *s, intptr_t *len, int flags) {
	std::lock_guard<std::mutex> g(searchLock);
	if (!d->HasCaseFolder())
		d->SetCaseFolder(std::make_unique<CaseFolderUnicode>());
	Sci::Position l = *len;
	Sci::Position pos;
	try {
		pos = d->FindText(minPos, maxPos, s, static_cast<Scintilla::FindOption>(flags), &l);
	} catch (std::exception &e) {
		g_exceptionMessage = e.what();
		pos = -2;
	}
	if (pos < -1)
		lastError = g_exceptionMessage;
	*len = l;
	return pos;
}

const char *npp_regex_error() { return lastError.c_str(); }

// Replaces [pos, pos + len) and returns the new length, or -1 on failure; regex mode expands back references of the last match.
intptr_t npp_doc_replace(Document *d, intptr_t pos, intptr_t len, const char *s, intptr_t slen, int regex) {
	try {
		std::string text(s, slen);
		if (regex) {
			Sci::Position l = slen;
			const char *p = d->SubstituteByPosition(s, &l);
			if (!p)
				return len;
			text.assign(p, l);
		}
		d->BeginUndoAction();
		d->DeleteChars(pos, len);
		const intptr_t n = d->InsertString(pos, text.data(), text.size());
		d->EndUndoAction();
		return n;
	} catch (std::exception &) {
		return -1;
	}
}

void npp_doc_undo_group(Document *d, int begin) {
	if (begin)
		d->BeginUndoAction();
	else
		d->EndUndoAction();
}

void npp_doc_undo(Document *d) { d->Undo(); }

Markings *npp_markings_new() { return new Markings(); }

// counts[i] is the number of segments on line i; pairs holds start and end of each segment.
SearchResultMarkings *npp_markings_set(Markings *m, const intptr_t *counts, intptr_t nlines, const intptr_t *pairs) {
	try {
		m->lines.assign(nlines, {});
		for (intptr_t i = 0; i < nlines; i++) {
			for (intptr_t k = 0; k < counts[i]; k++, pairs += 2)
				m->lines[i]._segmentPostions.emplace_back(pairs[0], pairs[1]);
		}
	} catch (std::exception &) {
		m->lines.clear();
	}
	m->s._length = static_cast<intptr_t>(m->lines.size());
	m->s._markings = m->lines.data();
	return &m->s;
}

}
