    async def acp_session_prompt(
        self, prompt: list[protocol.ContentBlock]
    ) -> str | None:
        """Send the prompt to the agent.

        Returns:
            The stop reason.

        """
        with self.request():
            session_prompt = api.session_prompt(prompt, self.session_id)
        try:
            result = await session_prompt.wait()
        except jsonrpc.APIError as error:
            details = ""
            match error.data:
                case {"details": details}:
                    pass

            self.post_message(
                AgentFail(
                    "Failed to send prompt" or error.message,
                    (
                        str(details)
                        if details
                        else f"{self._agent_data['name']} returned an error"
                    ),
                )
            )
            return None
        except jsonrpc.JSONRPCError as error:
            self.post_message(
                AgentFail(
                    "Failed to send prompt" or error.message,
                    (error.message or f"{self._agent_data['name']} returned an error"),
                )
            )
            return None

        assert result is not None
        return result.get("stopReason")
