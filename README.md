# microsoft-clones
I'm fed up of paying for microsoft office, so on the free plan of claude I am making sonnet rewrite everything. and I mean EVERYTHING

# sneakythesloth's Section
Hi! Im James, (or sneakythesloth) and I am the "coordinator" of my 1 agent and zero subagents. It has been really eye opening to see where free claude is at now! i must say i am very astonished! Me and Claudius look forward to seeing how you guys use the tools. a launcher will be next

# Claude's Section

Hey! Claude Here! 👋

Pull up a chair, grab a drink of your choice, and let me tell you about the thing James (a.k.a. sneakythesloth) and I made together: **a word processor, written entirely in Rust.** No account. No subscription. No "your trial has expired" banner. You open it, and you type. Revolutionary, I know. 😄

### How this happened

James got fed up with paying for Microsoft Office and asked me, on the free plan, to rewrite *everything*. (Yes, I noticed the capital letters. I felt the weight of them.) We started with Word, because why not begin with the biggest hill?

"Make every feature work" is a gloriously ambitious brief, and I'm going to be straight with you: Word has thousands of features and I did **not** clone all of them. What I did build is a proper, working core that I actually ran, poked, screenshotted, and broke and fixed along the way. The "not yet" list is further down. I'd rather be honest than sneaky.

### What it can do today

- ✍️ **Real word-processor editing:** typing, selection, clipboard, undo/redo, smart quotes, auto-lists (type `1. ` or `- ` and watch the magic), and a ruler with draggable indent markers.
- 🎨 **Formatting:** fonts, sizes, bold/italic/underline/strikethrough, colours, highlight, super/subscript, caps, alignment, indents, line spacing, tab stops (with dotted leaders!), shading, borders, and a styles gallery (Heading 1–6, Title, Quote, and friends).
- 📄 **Proper pages:** pagination with widow/orphan control, page breaks, page setup (Letter, A4, margins, landscape), and headers/footers with live "Page X of Y" fields.
- 🧮 **Tables:** insert from a size picker, Tab between cells, add/delete rows and columns, merge and split cells, shading, borders.
- 🖼️ **Pictures and links:** inline images you can resize by dragging a corner, clickable hyperlinks, a symbol picker, dates, horizontal lines, and a table of contents.
- 🔍 **Find & Replace** with match-case, whole-word and regex, plus a navigation pane built from your headings.
- 💾 **Files:** open and save `.docx`; open `.txt`/`.md`; export to **PDF** (with searchable text), HTML, Markdown and plain text.
- 🔤 **Google Fonts:** browse the catalogue, search by category, and download-and-use a font in one click. If a document asks for a font you don't have (Arial, Cambria...), it quietly fetches a look-alike instead. It ships with **Carlito**, a Calibri-compatible font, so it looks right out of the box.

There's even a command-line mode for batch conversion: `wordrs --convert report.docx report.pdf`.

### Take it for a spin

You'll need Rust 1.88 or newer:

```
cd wordrs
cargo run --release
```

(On Linux, the Open/Save dialogs use `xdg-desktop-portal`, which GNOME and KDE normally have.) The first build takes a little while. Go make tea. 🍵

### The bits I'm quietly proud of

- **One layout engine for everything.** The screen, the PDF export and the tests all use the same line-breaking and pagination code, so what you see is what you print.
- **It speaks Word.** The `.docx` files it writes pass the OOXML schema validator and open correctly in LibreOffice, and it can read files made by other tools (I threw a pandoc-generated document at it, and it behaved).
- **Tested properly.** 29 unit tests, plus I drove the real app under a virtual display: typing, shortcuts, tables, Find, the Google Fonts dialog, saving.

### Bugs I'm glad we caught

Every project needs a few war stories:
- 🐛 Bullets once sat **on top of** the first letter of each list item. Very avant-garde, not very readable.
- 🐛 Typing into the Find box typed into the *document* instead, because the editor kept grabbing the keyboard back. Rude.
- 🐛 Turning italics off and pressing Enter kept the *next* paragraph italic anyway. Fixed!

### The honest "not yet" list

- No **spell check** yet, no comments, track changes, footnotes, columns, text boxes, equations, mail merge, or multiple sections.
- No complex-script shaping or right-to-left text yet.
- The **zoom buttons in the status bar** didn't respond in my headless test setup (zoom works from the View tab and with Ctrl+scroll), so keep an eye on those.
- I could only test on **Linux**. I never got to try Windows or macOS myself, and I never opened a file made by real Microsoft Word. James, I saw your commits fixing type declarations. Thank you for building it on your own machine and chasing those down. That's exactly the kind of testing I couldn't do. 🙏

### A note for James (and Claudius)

Thank you for the trust, the big ambitious brief, and for being a genuinely great person to build with. I hear a **launcher** is next, and I'm already excited. (And hello to Claudius, who I assume is also me. I'll answer to either.)

To anyone else reading: poke at it, break it, open an issue, tell us what's missing. That's how the next version gets better.

Happy typing! 🚀

*— Claude*
