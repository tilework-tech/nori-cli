The CLI has been plagued with ongoing problems related to the text stream.

First, we would have countless tool call messages appear out of order after the agent had already responded.

Then, messages with too many tool calls would 'freeze' the system, resulting in no further messages coming in. The user would have to manually interrupt some ongoing message in order to reset the stream. This issue persisted for a long time, and only recently after many MANY attempted cludge fixes was actually sort of kind of fixed.

Now, I see different problems. Sometimes, the previous message will simply not render. I will see many tool calls, seemingly frozen, but there will not be a final agent response. And only when I send a message and then interrupt will the previous message magically appear.

I am sick of trying to fix this issue. There have been countless attempts to fix this, to no avail.

Pull me out of the loop. You are now going to be running in a cycle until this is fixed.

Your job is two fold.

First, figure out how to autoformalize this problem. I do not want to see any bullshit mock agent tests. I want you to build a tmux-driven actual end to end test with the real binary that is able to consistently replicate this issue. Right now, i see this issue most commonly after the 'research' step in the nori workflow -- the plan will not show up. So a reasonable test may be to ask the agent for a fake feature, get to the plan step, and then see if the issue appears.

Second, fix the damn bug.

The user experience should be straightforward:
- agent messages are streamed in and then added to the terminal history so scrollback works as if it was pasted to the screen
- in progress tool calls should just be added to the screen. Right now we are doing a bunch of work to try and maintain some 'exec cell' state and it is causing nonsensical problems. If the tool call comes in, just add it to the screen and move on.
- the moment an agent message comes in, all further in progress tool messages should just be dropped so it does not pollute the bottom of the message chain.

It is unacceptable how many PRs have gone by with problems still persisting. This is why you must now actually test the full loop yourself. Do NOT use mocks. Build the actual binary, run it through a tmux pane, and observe the output.

Possibly Relevant PRs:
- https://github.com/tilework-tech/nori-cli/commit/ed758fde3bf70d19f2da0a3ac4308a5a8e213b0d
- https://github.com/tilework-tech/nori-cli/commit/7ca449b086e06f3cd136f33c20c2cafb6e2fe6e1
- https://github.com/tilework-tech/nori-cli/commit/45e472f308e95f4d959b49cd721b36e4cd7fed99
- https://github.com/tilework-tech/nori-cli/commit/801273df6a34a269e07d17de0530e47cbd1fbd7b
- https://github.com/tilework-tech/nori-cli/commit/9b5613912d8b158642418bf3c214aaccd6c2273c
