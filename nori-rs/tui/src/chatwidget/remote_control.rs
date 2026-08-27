use super::*;

impl ChatWidget {
    pub(crate) fn open_remote_control_confirmation(&mut self, addr: std::net::SocketAddr) {
        let header = ColumnRenderable::with(vec![
            Box::new(Line::from(format!("Expose remote control on {addr}?")).bold())
                as Box<dyn Renderable>,
            Box::new(
                Paragraph::new(Line::from(
                    "This grants unauthenticated ACP access to any client that can reach this address."
                        .red(),
                ))
                .wrap(Wrap { trim: false }),
            ),
        ]);
        let accept: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::ConfirmRemoteControlExplicit(addr));
        })];
        let items = vec![
            SelectionItem {
                name: "Enable remote control".to_string(),
                description: Some("Allow this address for the current run only".to_string()),
                actions: accept,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Cancel".to_string(),
                description: Some("Keep remote control unchanged".to_string()),
                actions: Vec::new(),
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }
}
