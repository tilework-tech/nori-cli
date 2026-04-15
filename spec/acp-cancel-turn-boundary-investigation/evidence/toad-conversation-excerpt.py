    @work
    async def send_prompt_to_agent(self, prompt: str) -> None:
        if self.agent is not None:
            stop_reason: str | None = None
            self.busy_count += 1
            try:
                self.turn = "agent"
                stop_reason = await self.agent.send_prompt(prompt)
            except jsonrpc.APIError as error:
                from toad.widgets.markdown_note import MarkdownNote

                self.turn = "client"

                message = error.message or "no details were provided"

                await self.post(
                    MarkdownNote(
                        INTERNAL_EROR.replace("$ERROR", message),
                        classes="-stop-reason",
                    )
                )
            finally:
                self.busy_count -= 1
            self.call_later(self.agent_turn_over, stop_reason)

    async def agent_turn_over(self, stop_reason: str | None) -> None:
        """Called when the agent's turn is over.

        Args:
            stop_reason: The stop reason returned from the Agent, or `None`.
        """
        self.turn = "client"
        if self._agent_thought is not None and self._agent_thought.loading:
            await self._agent_thought.remove()
        if self._loading is not None:
            await self._loading.remove()
