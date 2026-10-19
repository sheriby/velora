use super::*;
use gpui_kit::component::{
    form::{Field, Form},
    group_box::{GroupBox, GroupBoxVariants},
    input::{InputEvent, InputState, NumberInput},
    searchable_list::{SearchableListItem, SearchableVec},
    select::{Select, SelectEvent, SelectState},
};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct PreferenceChoice {
    value: SharedString,
    label: SharedString,
    selector: SharedString,
}

impl PreferenceChoice {
    pub(super) fn new(
        value: impl Into<SharedString>,
        label: impl Into<SharedString>,
        selector: impl Into<SharedString>,
    ) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            selector: selector.into(),
        }
    }

    fn include_current(choices: &mut Vec<Self>, id: &str, value: &SharedString) -> bool {
        if choices.iter().any(|choice| &choice.value == value) {
            return false;
        }
        choices.push(Self::new(
            value.clone(),
            value.clone(),
            format!("{id}-current"),
        ));
        true
    }
}

impl SearchableListItem for PreferenceChoice {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &SharedString {
        &self.value
    }
    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let selector = self.selector.clone();
        div()
            .debug_selector(move || selector.to_string())
            .child(self.label.clone())
    }
}

type PreferenceSelect = SelectState<SearchableVec<PreferenceChoice>>;

struct ChoiceControl {
    state: Entity<PreferenceSelect>,
    choices: Vec<PreferenceChoice>,
    value: SharedString,
    _subscription: Subscription,
}

struct NumberControl {
    state: Entity<InputState>,
    value: f64,
    invalid: bool,
    _subscription: Subscription,
}

#[derive(Default)]
pub(super) struct PreferenceControls {
    locale: String,
    choices: BTreeMap<&'static str, ChoiceControl>,
    numbers: BTreeMap<&'static str, NumberControl>,
}

impl PreferenceControls {
    pub(super) fn sync_locale(&mut self, locale: &str) {
        if self.locale != locale {
            self.locale = locale.to_owned();
            self.choices.clear();
        }
    }

    pub(super) fn has_invalid_number(&self) -> bool {
        self.numbers.values().any(|control| control.invalid)
    }
}

impl PreferencesWindow {
    pub(super) fn boolean_field(
        id: &'static str,
        label: String,
        value: bool,
        cx: &mut Context<Self>,
        change: impl Fn(&mut Self, bool) + 'static,
    ) -> Switch {
        Switch::new(id)
            .accessibility_label(label)
            .checked(value)
            .on_change(cx.listener(move |preferences, next, _, cx| {
                change(preferences, *next);
                cx.notify();
            }))
    }

    pub(super) fn choice_field(
        &mut self,
        id: &'static str,
        label: String,
        value: impl Into<SharedString>,
        options: impl FnOnce(&Self) -> Vec<PreferenceChoice>,
        searchable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl Fn(&mut Self, &str, &mut Window, &mut Context<Self>) + 'static,
    ) -> Div {
        let value = value.into();
        let state = if let Some(control) = self.controls.choices.get_mut(id) {
            if control.value != value {
                control.value = value.clone();
                let added = PreferenceChoice::include_current(&mut control.choices, id, &value);
                if control.state.read(cx).selected_value() != Some(&value) {
                    control.state.update(cx, |state, cx| {
                        if added {
                            state.set_items(
                                SearchableVec::new(control.choices.clone()),
                                window,
                                cx,
                            );
                        }
                        state.set_selected_value(&value, window, cx);
                    });
                }
            }
            control.state.clone()
        } else {
            let mut choices = options(self);
            PreferenceChoice::include_current(&mut choices, id, &value);
            let selected = choices
                .iter()
                .position(|item| item.value == value)
                .map(|index| gpui_kit::component::IndexPath::default().row(index));
            let state = cx.new(|cx| {
                SelectState::new(SearchableVec::new(choices.clone()), selected, window, cx)
                    .searchable(searchable)
            });
            let subscription = cx.subscribe_in(
                &state,
                window,
                move |preferences,
                      _,
                      event: &SelectEvent<SearchableVec<PreferenceChoice>>,
                      window,
                      cx| {
                    if let SelectEvent::Confirm(Some(value)) = event {
                        change(preferences, value.as_ref(), window, cx);
                        cx.notify();
                    }
                },
            );
            self.controls.choices.insert(
                id,
                ChoiceControl {
                    state: state.clone(),
                    choices,
                    value: value.clone(),
                    _subscription: subscription,
                },
            );
            state
        };
        div()
            .debug_selector(move || id.to_string())
            .w(PREFERENCE_CHOICE_WIDTH)
            .max_w_full()
            .min_w_0()
            .child(
                Select::new(&state)
                    .id(id)
                    .accessibility_label(label)
                    .menu_max_h(rems(15.0))
                    .w_full(),
            )
    }

    pub(super) fn sync_choice_options(
        &mut self,
        id: &'static str,
        mut choices: Vec<PreferenceChoice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(control) = self.controls.choices.get_mut(id) else {
            return;
        };
        PreferenceChoice::include_current(&mut choices, id, &control.value);
        if control.choices == choices {
            return;
        }
        control.choices = choices.clone();
        control.state.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(choices), window, cx);
            state.set_selected_value(&control.value, window, cx);
        });
    }

    pub(super) fn number_field(
        &mut self,
        id: &'static str,
        label: String,
        value: f64,
        range: std::ops::RangeInclusive<f64>,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl Fn(&mut Self, f64) + 'static,
    ) -> AnyElement {
        let (state, invalid) = if let Some(control) = self.controls.numbers.get_mut(id) {
            if control.value != value {
                control.value = value;
                control.invalid = false;
                control.state.update(cx, |input, cx| {
                    input.set_value(value.to_string(), window, cx)
                });
            }
            (control.state.clone(), control.invalid)
        } else {
            let minimum = *range.start();
            let maximum = *range.end();
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value.to_string())
                    .min(minimum)
                    .max(maximum)
                    .step(1.0)
                    .validate(|text, _| text.chars().all(|character| character.is_ascii_digit()))
            });
            let subscription =
                cx.subscribe(&state, move |preferences, state, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let parsed = state
                        .read(cx)
                        .value()
                        .parse::<f64>()
                        .ok()
                        .filter(|value| value.is_finite());
                    if let Some(control) = preferences.controls.numbers.get_mut(id) {
                        control.invalid = parsed.is_none();
                        if let Some(value) = parsed {
                            let value = value.clamp(minimum, maximum);
                            control.value = value;
                            change(preferences, value);
                        }
                    }
                    cx.notify();
                });
            self.controls.numbers.insert(
                id,
                NumberControl {
                    state: state.clone(),
                    value,
                    invalid: false,
                    _subscription: subscription,
                },
            );
            (state, false)
        };
        div()
            .id(id)
            .test_support()
            .role(Role::Group)
            .aria_label(label)
            .debug_selector(move || id.to_string())
            .w(PREFERENCE_NUMBER_WIDTH)
            .max_w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(NumberInput::new(&state).w_full())
            .when(invalid, |field| {
                field.child(
                    div()
                        .text_xs()
                        .text_color(
                            cx.global::<ThemeManager>()
                                .current()
                                .colors
                                .dialog_danger_button_bg,
                        )
                        .child(
                            cx.global::<I18nManager>()
                                .strings()
                                .preferences_number_invalid
                                .clone(),
                        ),
                )
            })
            .into_any_element()
    }

    pub(super) fn preference_row(label: String, control: impl IntoElement) -> Field {
        Field::new().label(label).child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_end()
                .child(control),
        )
    }

    pub(super) fn preference_group_is_horizontal(window: &Window, control_width: Rems) -> bool {
        // 与实际内容列和 GroupBox 内边距共用尺寸，不能按整窗宽度猜测控件是否放得下。
        let inner_width = (window.viewport_size().width - px(PREFERENCE_SIDEBAR_WIDTH))
            .min(px(PREFERENCE_CONTENT_WIDTH))
            - px(PREFERENCE_CONTENT_PADDING * 2.0)
            - window.rem_size() * 2.0;
        let required_width = PREFERENCE_LABEL_WIDTH.to_pixels(window.rem_size())
            + control_width.to_pixels(window.rem_size())
            + window.rem_size();
        inner_width >= required_width
    }

    pub(super) fn preference_group(
        id: &'static str,
        label: String,
        fields: Vec<Field>,
        window: &Window,
        control_width: Rems,
    ) -> AnyElement {
        let form = if Self::preference_group_is_horizontal(window, control_width) {
            Form::horizontal().label_width(PREFERENCE_LABEL_WIDTH.to_pixels(window.rem_size()))
        } else {
            Form::vertical()
        };
        GroupBox::new()
            .id(id)
            .fill()
            .when(!label.is_empty(), |group| group.title(label))
            .w_full()
            .child(form.children(fields))
            .into_any_element()
    }
}
