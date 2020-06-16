/*! Module handling mouse events

TODO: Keyboard events
*/

use crate::abi::datastructures::{ComponentRef, MouseEvent};
use crate::EvaluationContext;
use euclid::default::Vector2D;

pub fn process_mouse_event(component: ComponentRef<'_>, event: MouseEvent) {
    let offset = Vector2D::new(0., 0.);

    crate::item_tree::visit_items(
        component,
        |component, item, offset| {
            let context = &EvaluationContext { component };

            let geom = item.geometry(context);
            let geom = geom.translate(*offset);

            if geom.contains(event.pos) {
                let mut event2 = event.clone();
                event2.pos -= geom.origin.to_vector();
                item.input_event(event2, context);
            }

            geom.origin.to_vector()
        },
        offset,
    );
}
