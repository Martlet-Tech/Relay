"""Relay terminal UI — prompt_toolkit Application wrapper."""

import threading
from prompt_toolkit.application import Application
from prompt_toolkit.buffer import Buffer
from prompt_toolkit.key_binding import KeyBindings
from prompt_toolkit.layout import HSplit, Layout, VSplit, Window
from prompt_toolkit.layout.controls import BufferControl, FormattedTextControl
from prompt_toolkit.styles import Style
from prompt_toolkit.widgets import Frame


class RelayUI:
    """Full-screen terminal UI using prompt_toolkit Application."""

    def __init__(self, cfg, session, username, env, on_submit=None):
        self.cfg = cfg
        self.session = session
        self.username = username
        self.env = env
        self.on_submit = on_submit

        self._lock = threading.Lock()
        self._processing = False
        self._chat_fragments = []
        self._enter_sends = cfg.enter_sends

        # Input buffer
        self.input_buffer = Buffer(multiline=not cfg.enter_sends)

        self._build_layout()
        self._build_keybindings()
        self._build_style()

        self.app = Application(
            layout=Layout(self.root_container),
            key_bindings=self.kb,
            style=self.style,
            mouse_support=False,
            full_screen=True,
        )

    # ── Public API ──

    def append(self, fragments):
        """Append formatted fragments: [(style, text), ...]. Thread-safe."""
        with self._lock:
            self._chat_fragments.extend(fragments)
        self._scroll_to_bottom()
        self.app.invalidate()

    def append_text(self, text, style=""):
        """Append plain text (optionally with style class)."""
        self.append([(style, text)])

    def append_line(self, text="", style=""):
        """Append a line of text (with trailing newline)."""
        self.append([(style, text + "\n")])

    def append_ansi(self, text):
        """Append text containing ANSI escape codes. Thread-safe."""
        from prompt_toolkit.formatted_text import ANSI as _ANSI
        from prompt_toolkit.formatted_text import to_formatted_text
        fragments = to_formatted_text(_ANSI(text + "\n"))
        self.append(fragments)

    def status(self, msg):
        """Temporarily show status in toolbar."""
        self.app.invalidate()

    def set_processing(self, busy):
        """Enable/disable input during processing."""
        self._processing = busy
        self.app.invalidate()

    def run(self):
        """Start the application (blocks until exit)."""
        self._render_banner()
        self.app.run()

    def exit(self):
        """Exit the application."""
        self.app.exit()

    # ── Layout ──

    def _build_layout(self):
        from prompt_toolkit.layout import Dimension
        from prompt_toolkit.layout.margins import ScrollbarMargin

        # Chat display — formatted text control for colors/styles
        self.chat_window = Window(
            content=FormattedTextControl(self._get_chat_text),
            wrap_lines=True,
            right_margins=[ScrollbarMargin()],
        )

        # Input area: prompt prefix + editable buffer
        input_vsplit = VSplit([
            Window(
                content=FormattedTextControl(lambda: [("", "  │ > ")]),
                width=6,
                dont_extend_width=True,
            ),
            Window(
                content=BufferControl(buffer=self.input_buffer),
                wrap_lines=True,
                height=Dimension(min=1, max=8),
            ),
        ])

        # Frame adds the box border — contrain so it starts at 1 line
        input_frame = Frame(input_vsplit, height=Dimension(min=3, max=8))

        # Bottom toolbar
        toolbar_window = Window(
            content=FormattedTextControl(self._get_toolbar_text),
            height=1,
            style="class:toolbar",
        )

        self.root_container = HSplit([
            self.chat_window,    # chat history — takes remaining space
            input_frame,         # bordered input area (grows with content)
            toolbar_window,      # status bar
        ])

    # ── Key bindings ──

    def _build_keybindings(self):
        self.kb = KeyBindings()

        if self._enter_sends:
            @self.kb.add("enter")
            def _submit(event):
                if not self._processing:
                    text = self.input_buffer.text.strip()
                    if text:
                        self.input_buffer.text = ""
                        if self.on_submit:
                            self.on_submit(text)

            @self.kb.add("escape", "enter")
            def _newline(event):
                self.input_buffer.insert_text("\n")
        else:
            @self.kb.add("enter")
            def _newline(event):
                self.input_buffer.insert_text("\n")

            @self.kb.add("escape", "enter")
            def _submit(event):
                if not self._processing:
                    text = self.input_buffer.text.strip()
                    if text:
                        self.input_buffer.text = ""
                        if self.on_submit:
                            self.on_submit(text)

        @self.kb.add("c-c")
        def _interrupt(event):
            if not self._processing:
                self.exit()

    # ── Style ──

    def _build_style(self):
        self.style = Style.from_dict({
            "toolbar": "bg:#005080 fg:#ffffff",
        })

    def _get_toolbar_text(self):
        status = " ● processing" if self._processing else ""
        return [("class:toolbar", f" Relay | {self.cfg.model}{status} ")]

    # ── Internal ──

    def _get_chat_text(self):
        return self._chat_fragments[:]

    def _scroll_to_bottom(self):
        self.chat_window.vertical_scroll = 10 ** 9

    def _render_banner(self):
        """Render initial banner into chat."""
        from env_detect import detect_environment

        env = self.env
        cfg = self.cfg
        username = self.username

        avail = ", ".join(
            k for k in ("git", "node", "npm", "cargo", "go", "make") if env.get(k)
        )
        os_ver = env.get("os", "")
        if env.get("os_version"):
            os_ver += f" ({env['os_version']})"

        lines = [
            f"User:  {username}",
            f"Model: {cfg.model}",
            f"OS:    {os_ver}",
            f"Shell: {env.get('default_shell', '?')}",
            f"CWD:   {env.get('cwd', '?')}",
        ]
        if avail:
            lines.append(f"Tools: {avail}")

        cmds = "/exit  /clear  /model <name>  /tools  /tokens"
        w = max(max(len(l) for l in lines + [cmds]) + 4, 54)
        p = w - 2
        hr = "+" + "-" * (w - 2) + "+"

        self.append_line()
        self.append_line(f"  {hr}")
        self.append_line(f"  |{'relay':^{p}}|")
        self.append_line(f"  {hr}")
        for l in lines:
            self.append_line(f"  | {l:<{p-1}}|")
        self.append_line(f"  {hr}")
        self.append_line(f"  | {cmds:<{p-1}}|")
        self.append_line(f"  {hr}")
        self.append_line()

    def handle_submit(self, text):
        """Process user text input (called from main)."""
        self.set_processing(True)
        import asyncio

        asyncio.get_event_loop().run_in_executor(
            None, self._run_processing, text
        )

    def _run_processing(self, text):
        """Run synchronous processing in a thread."""
        from chat import process_turn

        try:
            process_turn(self.cfg, self.session, self.username, ui=self)
        finally:
            self._processing = False
            self.app.invalidate()
