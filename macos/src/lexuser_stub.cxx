// SPDX-License-Identifier: GPL-3.0-or-later
// LexUser.cxx needs windows.h; user defined languages are out of scope for slice 1.
#include <cassert>
#include <string>
#include "ILexer.h"
#include "Scintilla.h"
#include "SciLexer.h"
#include "WordList.h"
#include "LexAccessor.h"
#include "Accessor.h"
#include "LexerModule.h"

using namespace Lexilla;

static void ColouriseNothing(Sci_PositionU, Sci_Position, int, WordList *[], Accessor &) {}

extern const LexerModule lmUserDefine(SCLEX_USER, ColouriseNothing, "user");
