// Layout prototype for the Qt Widgets parity work (RM-152).
//
// This is not wired to the Rust model yet. It reproduces the native
// AppCSXCAD window structure (menus, text toolbar, the three docks, tree
// columns, status bar) so the port can be compared against
// doc-src/concepts/images/appcsxcad.png before the model bridge lands.
#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QDockWidget>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMainWindow>
#include <QMenuBar>
#include <QPainter>
#include <QPushButton>
#include <QSlider>
#include <QStatusBar>
#include <QToolBar>
#include <QTreeWidget>
#include <QVBoxLayout>
#include <QWidget>

class Viewport : public QWidget {
public:
    Viewport(QWidget *parent = nullptr) : QWidget(parent) { setMinimumSize(400, 300); }

protected:
    void paintEvent(QPaintEvent *) override {
        QPainter painter(this);
        painter.fillRect(rect(), Qt::white);
        painter.setRenderHint(QPainter::Antialiasing);
        painter.setPen(QPen(Qt::darkGreen, 2));
        painter.drawLine(40, height() - 40, 40, height() - 110);
        painter.drawLine(40, height() - 40, 110, height() - 40);
        painter.setPen(Qt::darkGreen);
        painter.drawText(30, height() - 120, "Y");
        painter.setPen(Qt::red);
        painter.drawText(120, height() - 35, "X");
    }
};

static QAction *textAction(QToolBar *bar, const QString &text) {
    auto *action = bar->addAction(text);
    action->setToolTip(text);
    return action;
}

int main(int argc, char **argv) {
    QApplication app(argc, argv);
    QMainWindow window;
    window.setWindowTitle("AppCSXCAD");
    window.resize(800, 520);

    auto *file = window.menuBar()->addMenu("&File");
    file->addAction("New");
    file->addAction("Load");
    file->addAction("Save");
    file->addAction("Save As");
    file->addAction("Export");
    file->addSeparator();
    file->addAction("Exit");
    window.menuBar()->addMenu("&Info");

    auto *toolbar = window.addToolBar("Objects");
    toolbar->setMovable(false);
    toolbar->setToolButtonStyle(Qt::ToolButtonTextOnly);
    textAction(toolbar, "homo");
    textAction(toolbar, "inhomo");
    textAction(toolbar, "increase Disc");
    textAction(toolbar, "detect edges");
    textAction(toolbar, "CollapseAll");
    textAction(toolbar, "ExpandAll");
    toolbar->addSeparator();
    for (const char *name : {"Material", "Metal", "Electrode", "ProbeBox", "ResBox", "DumpBox"}) {
        textAction(toolbar, name);
    }
    toolbar->addSeparator();
    for (const char *name : {"Box", "MultiBox", "Sphere", "Cylinder", "Polygon", "User Defined"}) {
        textAction(toolbar, name);
    }
    toolbar->addSeparator();
    for (const char *name : {"YZ", "ZX", "2D", "3D"}) {
        textAction(toolbar, name);
    }

    window.setCentralWidget(new Viewport(&window));

    auto *properties = new QDockWidget("Properties and Structures", &window);
    auto *tree = new QTreeWidget(properties);
    tree->setHeaderLabels({"Properties / Primitives", "Vis"});
    tree->setColumnWidth(1, 40);
    const char *rows[][2] = {
        {"Excitation::excitation", ""}, {"  Box - ID: 1", ""},
        {"Excitation::excitation-weight-file", ""}, {"  Box - ID: 2", ""},
        {"ProbeBox::probe", ""}, {"  Box - ID: 3", ""},
        {"ProbeBox::probe-mode-file", ""}, {"  Box - ID: 4", ""},
        {"DumpBox::dump", ""}, {"  Box - ID: 5", ""},
    };
    for (const auto &row : rows) {
        auto *item = new QTreeWidgetItem(tree, {row[0], row[1]});
        item->setCheckState(0, Qt::Checked);
    }
    properties->setWidget(tree);
    window.addDockWidget(Qt::LeftDockWidgetArea, properties);

    auto *grid = new QDockWidget("Rectilinear Grid", &window);
    auto *gridBox = new QWidget(grid);
    auto *gridLayout = new QVBoxLayout(gridBox);
    auto *gridTable = new QWidget(gridBox);
    auto *gridRows = new QGridLayout(gridTable);
    gridRows->addWidget(new QLabel("Min"), 0, 0);
    gridRows->addWidget(new QLabel("Max Lines"), 0, 1);
    for (int row = 0; row < 3; ++row) {
        gridRows->addWidget(new QLabel(QString("XYZ")[row]), row + 1, 0);
        auto *value = new QLineEdit("0", gridTable);
        value->setMaximumWidth(60);
        gridRows->addWidget(value, row + 1, 1);
        auto *lines = new QLineEdit("0", gridTable);
        lines->setMaximumWidth(60);
        gridRows->addWidget(lines, row + 1, 2);
        gridRows->addWidget(new QPushButton("Edit", gridTable), row + 1, 3);
    }
    gridLayout->addWidget(gridTable);
    auto *unit = new QHBoxLayout();
    unit->addWidget(new QLabel("Drawing unit [m]:"));
    unit->addWidget(new QLineEdit("1", gridBox));
    gridLayout->addLayout(unit);
    auto *opacity = new QHBoxLayout();
    opacity->addWidget(new QLabel("Grid opacity"));
    auto *slider = new QSlider(Qt::Horizontal, gridBox);
    opacity->addWidget(slider);
    gridLayout->addLayout(opacity);
    grid->setWidget(gridBox);
    window.addDockWidget(Qt::LeftDockWidgetArea, grid);

    auto *parameter = new QDockWidget("Parameter", &window);
    auto *parameterBox = new QWidget(parameter);
    auto *parameterLayout = new QVBoxLayout(parameterBox);
    parameterLayout->addWidget(new QListWidget(parameterBox));
    parameterLayout->addWidget(new QPushButton("New", parameterBox));
    parameter->setWidget(parameterBox);
    window.addDockWidget(Qt::LeftDockWidgetArea, parameter);

    window.statusBar()->showMessage("X: -180 Y: -610");

    window.show();
    app.processEvents();
    window.grab().save(QString::fromLocal8Bit(qgetenv("PARITY_SHOT").constData()));
    return 0;
}
